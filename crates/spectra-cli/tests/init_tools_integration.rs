//! `spectra init --tools` integration tests pinned to Spectra 3.0.0.
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

fn expected_stdout(root: &Path, spec_dir: &str, tools: &str) -> String {
    format!(
        "✓ Initialized at {}\nGenerated files for: {tools}\n",
        root.join(spec_dir).display()
    )
}

fn golden_rows(tools: &[&str]) -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/update-trees-3.0.0.tsv");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let tool = fields.next().unwrap();
            let relpath = fields.next().unwrap();
            let sha = fields.next().unwrap();
            tools
                .contains(&tool)
                .then(|| (relpath.to_string(), sha.to_string()))
        })
        .collect()
}

fn golden_tool_ids() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/update-trees-3.0.0.tsv");
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

fn assert_tool_files_match_golden(root: &Path, spec_dir: &str, tools: &[&str]) {
    let mut expected = golden_rows(tools);
    expected.sort();

    for (relpath, sha) in &expected {
        let bytes = std::fs::read(root.join(relpath))
            .unwrap_or_else(|error| panic!("missing {relpath}: {error}"));
        assert_eq!(sha256_hex(&bytes), *sha, "{relpath} bytes drifted");
    }

    let config_yaml = format!("{spec_dir}/config.yaml");
    let mut actual = Vec::new();
    collect_files(root, root, &mut actual);
    actual.retain(|path| path != ".gitignore" && path != ".spectra.yaml" && *path != config_yaml);
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

/// Golden byte-for-byte tests use `--dir docs/spectra` because the golden TSV
/// was captured with the v3.0.0 oracle's default spec_dir (`docs/spectra`).
const GOLDEN_SPEC_DIR: &str = "docs/spectra";

#[test]
fn single_tool_matches_the_update_golden_byte_for_byte() {
    let root = TempDir::new("init-tools-single");

    let out = run_init(&root, &["--tools", "claude", "--dir", GOLDEN_SPEC_DIR]);

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, GOLDEN_SPEC_DIR, "claude")
    );
    assert!(out.stderr.is_empty());
    assert_tool_files_match_golden(&root, GOLDEN_SPEC_DIR, &["claude"]);
}

#[test]
fn every_registered_tool_matches_the_update_golden_byte_for_byte() {
    let tools = golden_tool_ids();
    assert_eq!(tools.len(), 6, "golden tool registry drifted");

    for tool in tools {
        let root = TempDir::new(&format!("init-tools-golden-{tool}"));
        let out = run_init(&root, &["--tools", &tool, "--dir", GOLDEN_SPEC_DIR]);

        assert!(out.status.success(), "{tool}: init failed: {out:?}");
        assert_eq!(
            String::from_utf8(out.stdout.clone()).unwrap(),
            expected_stdout(&root, GOLDEN_SPEC_DIR, &tool)
        );
        assert!(out.stderr.is_empty(), "{tool}: unexpected stderr: {out:?}");
        assert_tool_files_match_golden(&root, GOLDEN_SPEC_DIR, &[&tool]);
    }
}

#[test]
fn comma_separated_tools_preserve_input_order_and_match_the_update_golden() {
    let root = TempDir::new("init-tools-comma");

    let out = run_init(
        &root,
        &["--tools", "cursor,claude", "--dir", GOLDEN_SPEC_DIR],
    );

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, GOLDEN_SPEC_DIR, "cursor, claude")
    );
    assert!(out.stderr.is_empty());
    assert_tool_files_match_golden(&root, GOLDEN_SPEC_DIR, &["claude", "cursor"]);
}

#[test]
fn repeated_tools_flags_are_equivalent_to_comma_separated_values() {
    let root = TempDir::new("init-tools-repeated");

    let out = run_init(
        &root,
        &[
            "--tools",
            "claude",
            "--tools",
            "cursor",
            "--dir",
            GOLDEN_SPEC_DIR,
        ],
    );

    assert!(out.status.success(), "init failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        expected_stdout(&root, GOLDEN_SPEC_DIR, "claude, cursor")
    );
    assert!(out.stderr.is_empty());
    assert_tool_files_match_golden(&root, GOLDEN_SPEC_DIR, &["claude", "cursor"]);
}

#[test]
fn unknown_tool_is_rejected_with_error() {
    let root = TempDir::new("init-tools-unknown");

    let out = run_init(&root, &["--tools", "definitely-not-a-tool"]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("Unsupported coding agent: definitely-not-a-tool"),
        "expected unsupported agent error, got: {stderr}"
    );
    assert!(
        stderr.contains("Supported agents:"),
        "error should list supported agents: {stderr}"
    );
}

#[test]
fn unknown_tool_mixed_with_a_valid_tool_still_rejects() {
    let root = TempDir::new("init-tools-mixed");

    let out = run_init(&root, &["--tools", "claude,bogus"]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("Unsupported coding agent: bogus"),
        "expected unsupported agent error, got: {stderr}"
    );
}

#[test]
fn space_separated_value_is_rejected_as_unknown() {
    let root = TempDir::new("init-tools-space");

    let out = run_init(&root, &["--tools", "claude cursor"]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("Unsupported coding agent: claude cursor"),
        "space-separated value should be rejected: {stderr}"
    );
}
