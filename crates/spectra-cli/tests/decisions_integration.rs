//! `spectra decisions` 端到端：預期輸出逐位元組取自 oracle 3.0.0 在同一 fixture（含相同
//! mtime）上的輸出（W12 probe p21）。
mod common;

use std::path::Path;

use common::{spectra, TempDir};

const FILES: &[(&str, &str)] = &[
    (".spectra.yaml", "spec_dir: openspec\n"),
    ("openspec/config.yaml", "schema: spec-driven\n"),
    (
        "openspec/changes/alpha/.openspec.yaml",
        "schema: spec-driven\ncreated: 2026-09-01\n",
    ),
    (
        "openspec/changes/alpha/design.md",
        "## Context\n\nctx\n\n### Not A Decision In Context\n\ntext\n\n## Decisions\n\n\
         ### Use SQLite\n\nWe pick SQLite because it is embedded.\nSecond line of rationale.\n\n\
         #### Sub heading\n\nsub text\n\n### Empty Rationale\n\n### Replace Cache\n\n\
         **Supersedes**: old-change / Use Redis\n\nWe now use an in-process cache.\n\n\
         ### Bad Ref\n\n**Supersedes**: ghost / Nothing\n\nPointing nowhere.\n\n\
         ## Risks / Trade-offs\n\n### Not A Decision In Risks\n\nr\n",
    ),
    ("openspec/changes/beta/.openspec.yaml", "created: 2026-02-10\n"),
    (
        "openspec/changes/beta/design.md",
        "## Decisions\n\n### Beta Choice\n\nBeta rationale mentions sqlite lowercase.\n",
    ),
    ("openspec/changes/gamma/proposal.md", "## Why\n\nno design\n"),
    (
        "openspec/changes/archive/2026-01-15-old-change/.openspec.yaml",
        "schema: spec-driven\ncreated: 2026-01-10\n",
    ),
    (
        "openspec/changes/archive/2026-01-15-old-change/design.md",
        "## Decisions\n\n### Use Redis\n\nRedis is fast.\n\n### Keep Logs\n\nLogs are kept for 30 days.\n",
    ),
];

/// 使用中 change 的 mtime（秒）：beta 最新、alpha 其次，決定 modified 排序。
const MTIMES: &[(&str, u64)] = &[
    ("openspec/changes/alpha", 1_000),
    ("openspec/changes/beta", 2_000),
    ("openspec/changes/gamma", 500),
];

fn fixture(label: &str) -> TempDir {
    let root = TempDir::new(label);
    for (rel, text) in FILES {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }
    std::fs::create_dir_all(root.join("openspec/specs")).unwrap();
    for (rel, secs) in MTIMES {
        for entry in std::fs::read_dir(root.join(rel)).unwrap().flatten() {
            let file = std::fs::File::options()
                .write(true)
                .open(entry.path())
                .unwrap();
            file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(*secs))
                .unwrap();
        }
    }
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

const EXPECTED_JSON: &str = "[\n  {\n    \"heading\": \"Beta Choice\",\n    \"change\": \"beta\",\n    \"date\": \"\",\n    \"rationale\": \"Beta rationale mentions sqlite lowercase.\",\n    \"supersedes\": null,\n    \"superseded\": false,\n    \"unresolvableSupersession\": null\n  },\n  {\n    \"heading\": \"Use SQLite\",\n    \"change\": \"alpha\",\n    \"date\": \"2026-09-01\",\n    \"rationale\": \"We pick SQLite because it is embedded.\\nSecond line of rationale.\\n\\n#### Sub heading\\n\\nsub text\",\n    \"supersedes\": null,\n    \"superseded\": false,\n    \"unresolvableSupersession\": null\n  },\n  {\n    \"heading\": \"Empty Rationale\",\n    \"change\": \"alpha\",\n    \"date\": \"2026-09-01\",\n    \"rationale\": \"\",\n    \"supersedes\": null,\n    \"superseded\": false,\n    \"unresolvableSupersession\": null\n  },\n  {\n    \"heading\": \"Replace Cache\",\n    \"change\": \"alpha\",\n    \"date\": \"2026-09-01\",\n    \"rationale\": \"**Supersedes**: old-change / Use Redis\\n\\nWe now use an in-process cache.\",\n    \"supersedes\": {\n      \"change\": \"old-change\",\n      \"heading\": \"Use Redis\"\n    },\n    \"superseded\": false,\n    \"unresolvableSupersession\": null\n  },\n  {\n    \"heading\": \"Bad Ref\",\n    \"change\": \"alpha\",\n    \"date\": \"2026-09-01\",\n    \"rationale\": \"**Supersedes**: ghost / Nothing\\n\\nPointing nowhere.\",\n    \"supersedes\": null,\n    \"superseded\": false,\n    \"unresolvableSupersession\": \"ghost / Nothing\"\n  },\n  {\n    \"heading\": \"Use Redis\",\n    \"change\": \"old-change\",\n    \"date\": \"2026-01-15\",\n    \"rationale\": \"Redis is fast.\",\n    \"supersedes\": null,\n    \"superseded\": true,\n    \"unresolvableSupersession\": null\n  },\n  {\n    \"heading\": \"Keep Logs\",\n    \"change\": \"old-change\",\n    \"date\": \"2026-01-15\",\n    \"rationale\": \"Logs are kept for 30 days.\",\n    \"supersedes\": null,\n    \"superseded\": false,\n    \"unresolvableSupersession\": null\n  }\n]\n";

const EXPECTED_HUMAN: &str = "Beta Choice\n  beta\nUse SQLite\n  alpha · 2026-09-01\nEmpty Rationale\n  alpha · 2026-09-01\nReplace Cache\n  alpha · 2026-09-01\n  supersedes old-change / Use Redis\nBad Ref\n  alpha · 2026-09-01\n  unresolvable supersedes: ghost / Nothing\nUse Redis\n  old-change · 2026-01-15\n  superseded by a later decision\nKeep Logs\n  old-change · 2026-01-15\n\n7 decisions\n";

#[test]
fn decisions_json_and_human_match_the_oracle() {
    let root = fixture("decisions-full");
    assert_eq!(
        run(&root, &["decisions", "--json"]),
        (0, EXPECTED_JSON.to_string(), String::new())
    );
    assert_eq!(
        run(&root, &["decisions"]),
        (0, EXPECTED_HUMAN.to_string(), String::new())
    );
}

#[test]
fn keyword_filters_after_supersession_is_resolved() {
    let root = fixture("decisions-keyword");
    assert_eq!(
        run(&root, &["decisions", "redis"]),
        (
            0,
            "Replace Cache\n  alpha · 2026-09-01\n  supersedes old-change / Use Redis\nUse Redis\n  old-change · 2026-01-15\n  superseded by a later decision\n\n2 decisions\n".to_string(),
            String::new()
        )
    );
    assert_eq!(
        run(&root, &["decisions", "nomatch"]),
        (0, "No decisions found.\n".to_string(), String::new())
    );
    assert_eq!(
        run(&root, &["decisions", "nomatch", "--json"]),
        (0, "[]\n".to_string(), String::new())
    );
}

#[test]
fn decisions_works_from_a_subdirectory_and_needs_an_initialized_project() {
    let root = fixture("decisions-subdir");
    let sub = root.join("deep/er");
    std::fs::create_dir_all(&sub).unwrap();
    assert_eq!(run(&sub, &["decisions", "--json"]).1, EXPECTED_JSON);

    let bare = TempDir::new("decisions-uninit");
    assert_eq!(
        run(&bare, &["decisions"]),
        (
            1,
            String::new(),
            "Error: Not initialized. Run 'spectra init' to initialize.\n".to_string()
        )
    );
}
