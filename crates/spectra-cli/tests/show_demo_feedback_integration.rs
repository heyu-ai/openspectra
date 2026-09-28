//! `show --deltas-only／-r／--item-type`、`demo`、`feedback` 端到端。預期輸出取自 oracle
//! 3.0.0 在同一 fixture 上的輸出（W12 probe p16／p17／p01／p18／p19）。
mod common;

use std::path::Path;

use common::{git, spectra, TempDir};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// W12 probe p16 的 fixture。
fn show_fixture(label: &str) -> TempDir {
    let root = TempDir::new(label);
    write(&root, ".spectra.yaml", "spec_dir: openspec\n");
    write(&root, "openspec/config.yaml", "schema: spec-driven\n");
    std::fs::create_dir_all(root.join("openspec/changes/archive")).unwrap();
    let c = "openspec/changes/ch";
    write(
        &root,
        &format!("{c}/.openspec.yaml"),
        "schema: spec-driven\ncreated: 2026-09-01\n",
    );
    write(&root, &format!("{c}/proposal.md"), "## Why\n\nwhy text\n");
    write(
        &root,
        &format!("{c}/design.md"),
        "## Decisions\n\n### D\n\nr\n",
    );
    write(&root, &format!("{c}/tasks.md"), "## 1. T\n\n- [ ] 1.1 x\n");
    write(
        &root,
        &format!("{c}/specs/cap-a/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n",
    );
    write(
        &root,
        &format!("{c}/specs/cap-b/spec.md"),
        "## REMOVED Requirements\n\n### Requirement: Old\n\n**Reason**: x\n",
    );
    write(&root, &format!("{c}/specs/cap-b/sub/notes.md"), "extra\n");
    write(
        &root,
        "openspec/specs/cap-a/spec.md",
        "# cap-a Specification\n\n## Purpose\n\nPurpose text.\n",
    );
    write(
        &root,
        "openspec/specs/cap-a/design-notes.md",
        "extra spec file\n",
    );
    write(
        &root,
        "openspec/changes/both/.openspec.yaml",
        "schema: spec-driven\ncreated: 2026-09-01\n",
    );
    write(
        &root,
        "openspec/changes/both/proposal.md",
        "## Why\n\nchange both\n",
    );
    write(
        &root,
        "openspec/specs/both/spec.md",
        "# both\n\n## Purpose\n\np\n",
    );
    root
}

fn run(root: &Path, args: &[&str]) -> (i32, String, String) {
    let out = spectra()
        .args(args)
        .env("NO_COLOR", "1")
        .current_dir(root)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

const CH_HUMAN: &str = "Change: ch\nSchema: spec-driven\nCreated: 2026-09-01\n\n--- Proposal ---\n## Why\n\nwhy text\n\n\n--- Delta Specs ---\n  cap-a/spec.md\n  cap-b/spec.md\n  cap-b/sub/notes.md\n";

const CH_JSON: &str = "{\n  \"created\": \"2026-09-01\",\n  \"deltaSpecs\": [\n    \"cap-a/spec.md\",\n    \"cap-b/spec.md\",\n    \"cap-b/sub/notes.md\"\n  ],\n  \"design\": \"## Decisions\\n\\n### D\\n\\nr\\n\",\n  \"name\": \"ch\",\n  \"proposal\": \"## Why\\n\\nwhy text\\n\",\n  \"schema\": \"spec-driven\",\n  \"tasks\": \"## 1. T\\n\\n- [ ] 1.1 x\\n\"\n}\n";

const CAP_A_HUMAN: &str = "Spec: cap-a\n\n--- design-notes.md ---\nextra spec file\n\n\n--- spec.md ---\n# cap-a Specification\n\n## Purpose\n\nPurpose text.\n\n";

#[test]
fn deltas_only_and_requirements_are_accepted_but_inert() {
    let root = show_fixture("show-inert");
    for args in [
        &["show", "ch", "--deltas-only"][..],
        &["show", "ch", "-r"],
        &["show", "ch", "--requirements"],
        &["show", "ch", "--deltas-only", "-r"],
        &["show", "ch", "--item-type", "change"],
    ] {
        assert_eq!(
            run(&root, args),
            (0, CH_HUMAN.to_string(), String::new()),
            "{args:?}"
        );
    }
    for args in [
        &["show", "ch", "--deltas-only", "--json"][..],
        &["show", "ch", "-r", "--json"],
        &["show", "ch", "--deltas-only", "-r", "--json"],
    ] {
        assert_eq!(
            run(&root, args),
            (0, CH_JSON.to_string(), String::new()),
            "{args:?}"
        );
    }
    for args in [
        &["show", "cap-a", "-r"][..],
        &["show", "cap-a", "--deltas-only"],
        &["show", "cap-a", "--item-type", "spec"],
    ] {
        assert_eq!(
            run(&root, args),
            (0, CAP_A_HUMAN.to_string(), String::new()),
            "{args:?}"
        );
    }
}

#[test]
fn item_type_selects_the_kind_and_reports_oracle_errors() {
    let root = show_fixture("show-item-type");
    assert_eq!(
        run(&root, &["show", "both", "--item-type", "spec", "--json"]),
        (
            0,
            "{\n  \"files\": [\n    {\n      \"content\": \"# both\\n\\n## Purpose\\n\\np\\n\",\n      \"name\": \"spec.md\"\n    }\n  ],\n  \"name\": \"both\"\n}\n".to_string(),
            String::new()
        )
    );
    assert_eq!(
        run(&root, &["show", "both"]).1,
        "Change: both\nSchema: spec-driven\nCreated: 2026-09-01\n\n--- Proposal ---\n## Why\n\nchange both\n\n"
    );
    let err = |args: &[&str], msg: &str| {
        assert_eq!(
            run(&root, args),
            (1, String::new(), format!("Error: {msg}\n")),
            "{args:?}"
        );
    };
    err(
        &["show", "ch", "--item-type", "spec"],
        "Spec 'ch' not found.",
    );
    err(
        &["show", "cap-a", "--item-type", "change"],
        "Change 'cap-a' not found.",
    );
    err(
        &["show", "ch", "--item-type", "bogus"],
        "Unknown type: bogus. Use 'change' or 'spec'.",
    );
    err(
        &["show", "ghost", "--item-type", "Change"],
        "Unknown type: Change. Use 'change' or 'spec'.",
    );
    err(
        &["show", "ch", "--item-type="],
        "Unknown type: . Use 'change' or 'spec'.",
    );
    err(
        &["show", "ghost", "--deltas-only"],
        "Item 'ghost' not found as a change or spec.",
    );
}

#[test]
fn show_without_item_is_a_runtime_error() {
    // oracle 3.0.0：exit 1（不是 clap 的 exit 2），且先於 --item-type 的檢查。
    let root = show_fixture("show-no-item");
    for args in [&["show"][..], &["show", "--item-type", "bogus"]] {
        assert_eq!(
            run(&root, args),
            (
                1,
                String::new(),
                "Error: Please specify an item name.\n".to_string()
            ),
            "{args:?}"
        );
    }
}

#[test]
fn feedback_prints_locally_outside_any_project() {
    let root = TempDir::new("feedback");
    assert_eq!(
        run(&root, &["feedback", "hello", "--body", "multi\nline"]),
        (
            0,
            "Thank you for your feedback!\nMessage: hello\nDetails: multi\nline\n\nTo submit feedback, visit: https://github.com/kaochenlong/spectra-app/issues\n".to_string(),
            String::new()
        )
    );
    assert_eq!(
        run(&root, &["feedback", ""]).1,
        "Thank you for your feedback!\nMessage: \n\nTo submit feedback, visit: https://github.com/kaochenlong/spectra-app/issues\n"
    );
    // feedback 不寫任何檔案。
    assert_eq!(std::fs::read_dir(&*root).unwrap().count(), 0);
}

#[test]
fn demo_creates_one_sample_change_and_prints_its_path() {
    let root = TempDir::new("demo");
    write(&root, ".spectra.yaml", "spec_dir: openspec\n");
    write(&root, "openspec/config.yaml", "schema: custom-x\n");
    std::fs::create_dir_all(root.join("openspec/changes/archive")).unwrap();
    std::fs::create_dir_all(root.join("openspec/specs")).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "Howie"]);
    git(&root, &["config", "user.email", "howie@example.com"]);

    let (code, stdout, stderr) = run(&root, &["demo"]);
    assert_eq!((code, stderr.as_str()), (0, ""), "{stdout}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{stdout}");
    let name = lines[0]
        .strip_prefix("✓ Created demo change: ")
        .expect("first line");
    let theme = lines[1].strip_prefix("  Theme: ").expect("theme line");
    let dir = root.join("openspec/changes").join(name);
    assert_eq!(lines[2], format!("  Path: {}", dir.display()));
    let (adjective, pokemon) = name
        .strip_prefix("spx-")
        .and_then(|rest| rest.split_once('-'))
        .expect("spx-<adjective>-<pokemon>");
    assert!(spectra_core::demo::ADJECTIVES.contains(&adjective));
    assert!(spectra_core::demo::POKEMON.contains(&pokemon));
    let theme = spectra_core::demo::THEMES
        .iter()
        .find(|t| t.name == theme)
        .expect("known theme");
    assert_eq!(
        std::fs::read_to_string(dir.join("tasks.md")).unwrap(),
        theme.tasks
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("specs").join(theme.name).join("spec.md")).unwrap(),
        theme.spec
    );
    let meta = std::fs::read_to_string(dir.join(".openspec.yaml")).unwrap();
    assert!(
        meta.starts_with("schema: spec-driven\ncreated: ")
            && meta.ends_with("\ncreated_by: Howie <howie@example.com>\n"),
        "{meta}"
    );
    // oracle 的 demo 不寫 `.spectra/` 狀態（W12 probe p02）。
    assert!(!root.join(".spectra").exists());

    let bare = TempDir::new("demo-uninit");
    assert_eq!(
        run(&bare, &["demo"]),
        (
            1,
            String::new(),
            "Error: Not initialized. Run 'spectra init' to initialize.\n".to_string()
        )
    );
}
