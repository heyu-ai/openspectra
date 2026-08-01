//! `spectra init --tools` integration tests pinned to Spectra 2.3.1.
//! Tool-file bytes are checked against the existing update golden because the
//! oracle emits the same files through both commands.

mod common;

use std::path::Path;
use std::process::Output;

use common::{spectra, TempDir};
use sha2::{Digest, Sha256};

fn run_init(root: &Path, args: &[&str]) -> Output {
    spectra()
        .arg("init")
        .args(args)
        .arg("--no-color")
        .current_dir(root)
        .output()
        .unwrap()
}

fn expected_stdout(root: &Path, tools: &str) -> String {
    format!(
        "✓ Initialized at {}\nGenerated files for: {tools}\n",
        root.join("openspec").display()
    )
}

fn assert_only_default_init_files(root: &Path) {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    assert_eq!(
        files,
        [".gitignore", ".spectra.yaml", "openspec/config.yaml"]
    );
}

fn golden_rows(tools: &[&str]) -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/update-trees-2.3.1.tsv");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let tool = fields.next().unwrap();
            let relpath = fields.next().unwrap();
            let sha = fields.next().unwrap();
            let gate = fields.next().unwrap_or("Always");
            (tools.contains(&tool) && gate == "Always")
                .then(|| (relpath.to_string(), sha.to_string()))
        })
        .collect()
}

fn golden_tool_ids() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/update-trees-2.3.1.tsv");
    let mut tools = Vec::new();
    for line in std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let tool = line.split('\t').next().unwrap().to_string();
        if !tools.contains(&tool) {
            tools.push(tool);
        }
    }
    tools
}

/// 逐 byte 展開，不走 `{:x}`：`sha2` 0.11 起 digest 輸出型別由
/// `generic-array` 改為 `hybrid-array::Array`，後者沒有 `LowerHex` impl。
/// 手動編碼在 0.10 與 0.11 下都成立。
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn assert_tool_files_match_golden(root: &Path, tools: &[&str]) {
    let mut expected = golden_rows(tools);
    expected.sort();

    for (relpath, sha) in &expected {
        let bytes = std::fs::read(root.join(relpath))
            .unwrap_or_else(|error| panic!("missing {relpath}: {error}"));
        assert_eq!(sha256_hex(&bytes), *sha, "{relpath} bytes drifted");
    }

    let mut actual = Vec::new();
    collect_files(root, root, &mut actual);
    actual.retain(|path| {
        !matches!(
            path.as_str(),
            ".gitignore" | ".spectra.yaml" | "openspec/config.yaml"
        )
    });
    let expected_paths: Vec<_> = expected.into_iter().map(|(path, _)| path).collect();
    assert_eq!(actual, expected_paths);
}

fn collect_files(base: &Path, dir: &Path, files: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(base, &path, files);
        } else {
            files.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    files.sort();
}

#[test]
fn single_tool_matches_the_update_golden_byte_for_byte() {
    let root = TempDir::new("init-tools-single");

    let out = run_init(&root, &["--tools", "claude"]);

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, "claude")
    );
    assert!(out.stderr.is_empty());
    assert_tool_files_match_golden(&root, &["claude"]);
}

#[test]
fn every_registered_tool_matches_the_update_golden_byte_for_byte() {
    let tools = golden_tool_ids();
    assert_eq!(tools.len(), 23, "golden tool registry drifted");

    for tool in tools {
        let root = TempDir::new(&format!("init-tools-golden-{tool}"));
        let out = run_init(&root, &["--tools", &tool]);

        assert!(out.status.success(), "{tool}: init failed: {out:?}");
        assert_eq!(
            String::from_utf8(out.stdout.clone()).unwrap(),
            expected_stdout(&root, &tool)
        );
        assert!(out.stderr.is_empty(), "{tool}: unexpected stderr: {out:?}");
        assert_tool_files_match_golden(&root, &[&tool]);
    }
}

#[test]
fn comma_separated_tools_preserve_input_order_and_match_the_update_golden() {
    let root = TempDir::new("init-tools-comma");

    let out = run_init(&root, &["--tools", "cursor,claude"]);

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, "cursor, claude")
    );
    assert!(out.stderr.is_empty());
    assert_tool_files_match_golden(&root, &["claude", "cursor"]);
}

#[test]
fn repeated_tools_flags_are_equivalent_to_comma_separated_values() {
    let root = TempDir::new("init-tools-repeated");

    let out = run_init(&root, &["--tools", "claude", "--tools", "cursor"]);

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, "claude, cursor")
    );
    assert!(out.stderr.is_empty());
    assert_tool_files_match_golden(&root, &["claude", "cursor"]);
}

#[test]
fn unknown_tool_is_a_successful_silent_file_noop_but_is_echoed() {
    let root = TempDir::new("init-tools-unknown");

    let out = run_init(&root, &["--tools", "definitely-not-a-tool"]);

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, "definitely-not-a-tool")
    );
    assert!(out.stderr.is_empty());
    assert_only_default_init_files(&root);
}

#[test]
fn unknown_tool_mixed_with_a_valid_tool_does_not_block_the_valid_tool() {
    let root = TempDir::new("init-tools-mixed");

    let out = run_init(&root, &["--tools", "claude,bogus"]);

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, "claude, bogus")
    );
    assert!(out.stderr.is_empty());
    assert_tool_files_match_golden(&root, &["claude"]);
}

#[test]
fn space_separated_value_is_a_successful_silent_file_noop_and_is_echoed_verbatim() {
    let root = TempDir::new("init-tools-space");

    let out = run_init(&root, &["--tools", "claude cursor"]);

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, "claude cursor")
    );
    assert!(out.stderr.is_empty());
    assert_only_default_init_files(&root);
}
