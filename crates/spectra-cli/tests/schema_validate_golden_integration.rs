//! `spectra schema validate`／`schema fork` 對 oracle 3.0.0 golden 的重播比對（W7g）。
//!
//! `docs/reverse-engineering/golden/schema-validate-3.0.0.json` 由
//! `scripts/capture-schema-validate.py` 在 `tests/fixtures/schema_validate/<case>/`
//! 上實際執行 oracle 產生：每個案例是一個專案 schema（放在 `openspec/schemas/m/`），
//! 各跑 `schema validate m`、`--json`、`--verbose` 與 `schema fork m m2`，記錄
//! exit code、stdout、stderr。期望值全部來自 oracle 的真實輸出。
//!
//! 刻意分歧（owner 裁決 D11，見 `docs/reverse-engineering/schema.md`）不在 golden
//! 內調整，而是列在 [`divergence`]：每一項寫出 OpenSpectra 的字面輸出。清單是
//! ratchet——列了卻與 oracle 相同的案例也會失敗，修掉分歧時必須刪掉那一列。
//!
//! 截短保護：案例數必須等於 [`CASE_COUNT`]，fixture 目錄也必須正好是這些案例。

mod common;

use std::path::Path;

use serde_json::Value;

use common::{spectra, TempDir};

const CASE_COUNT: usize = 77;
const RUNS_PER_CASE: usize = 4;

fn golden() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/schema-validate-3.0.0.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("讀取 {path:?}：{e}"));
    serde_json::from_str(&raw).unwrap()
}

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema_validate")
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// 與 capture 腳本的 `make_project` 相同的專案骨架。
fn project(case: &str) -> TempDir {
    let root = TempDir::new(&format!("schema-validate-{case}"));
    std::fs::create_dir_all(root.join("openspec/changes/archive")).unwrap();
    std::fs::create_dir_all(root.join("openspec/specs")).unwrap();
    std::fs::write(root.join(".spectra.yaml"), "spec_dir: openspec\n").unwrap();
    std::fs::write(root.join("openspec/config.yaml"), "schema: spec-driven\n").unwrap();
    copy_tree(&fixture_root().join(case), &root.join("openspec/schemas/m"));
    root
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    exit_code: i64,
    stdout: String,
    stderr: String,
}

fn run(root: &Path, args: &[String]) -> Outcome {
    let output = spectra()
        .args(args)
        // 使用者層 schema 不能影響重播。
        .env("XDG_DATA_HOME", root.join("no-user-data"))
        .current_dir(root)
        .output()
        .unwrap();
    let scrub = |bytes: Vec<u8>| {
        String::from_utf8(bytes)
            .unwrap()
            .replace(&root.display().to_string(), "<ROOT>")
    };
    Outcome {
        exit_code: i64::from(output.status.code().unwrap()),
        stdout: scrub(output.stdout),
        stderr: scrub(output.stderr),
    }
}

fn ok(stdout: &str, stderr: &str) -> Outcome {
    Outcome {
        exit_code: 0,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

fn validate_failure(json: bool, error: &str) -> Outcome {
    if json {
        Outcome {
            exit_code: 1,
            stdout: format!(
                "{{\n  \"error\": {},\n  \"name\": \"m\",\n  \"valid\": false\n}}\n",
                serde_json::to_string(error).unwrap()
            ),
            stderr: format!("Error: Schema validation failed: {error}\n"),
        }
    } else {
        Outcome {
            exit_code: 1,
            stdout: String::new(),
            stderr: format!(
                "Schema 'm' is invalid: {error}\nError: Schema validation failed: {error}\n"
            ),
        }
    }
}

fn fork_failure(error: &str) -> Outcome {
    Outcome {
        exit_code: 1,
        stdout: String::new(),
        stderr: format!("Error: {error}\n"),
    }
}

const VALID_2: &str = "\u{2713} Schema 'm' is valid (2 artifacts)\n";
const VALID_2_JSON: &str = "{\n  \"artifactCount\": 2,\n  \"name\": \"m\",\n  \"valid\": true\n}\n";
const FORKED: &str = "\u{2713} Forked 'm' \u{2192} 'm2'\n";

/// 刻意分歧：`(case, run index)` → OpenSpectra 的字面輸出。run index 對應 golden 的
/// `runs`：0 = validate、1 = `--json`、2 = `--verbose`、3 = `fork m m2`。
fn divergence(case: &str, run: usize) -> Option<Outcome> {
    let json = run == 1;
    let fork = run == 3;
    let rejected = |error: &str| {
        if fork {
            fork_failure(error)
        } else {
            validate_failure(json, error)
        }
    };
    let warned = |warnings: &str| {
        if fork {
            ok(FORKED, warnings)
        } else if json {
            ok(VALID_2_JSON, warnings)
        } else {
            ok(VALID_2, warnings)
        }
    };
    match case {
        // D11-1：template 含 `..` 或絕對路徑一律拒絕（oracle：合法，fork 會寫到 templates/ 外）。
        "tpl-traversal" => Some(rejected(
            "Invalid schema: Artifact 'b' template '../x.md' must not contain '..'",
        )),
        "tpl-traversal-missing" => Some(rejected(
            "Invalid schema: Artifact 'b' template '../nope.md' must not contain '..'",
        )),
        // D11-7：generates 含 `..` 或絕對路徑同樣拒絕（oracle：合法，原樣當 outputPath）。
        "gen-absolute" => Some(rejected(
            "Invalid schema: Artifact 'b' generates '/abs/b.md' must be a relative path",
        )),
        "gen-traversal" => Some(rejected(
            "Invalid schema: Artifact 'b' generates '../b.md' must not contain '..'",
        )),
        // D11-2：缺檔或空檔仍合法，但在 stderr 警告（oracle：完全無聲）。
        "tpl-missing" => Some(warned(
            "Warning: Template 'b.md' for artifact 'b' is missing\n",
        )),
        "tpl-empty" => Some(warned(
            "Warning: Template 'b.md' for artifact 'b' is empty\n",
        )),
        "tpl-dir-missing" => Some(warned(
            "Warning: Template 'a.md' for artifact 'a' is missing\n\
             Warning: Template 'b.md' for artifact 'b' is missing\n",
        )),
        // D11-5：專案 schema 的 fork 照原樣複製整棵目錄，子目錄裡的 template 也複製
        // （oracle：`No such file or directory (os error 2)`，留下半成品）。
        "tpl-subdir" if fork => Some(ok(FORKED, "")),
        _ => None,
    }
}

#[test]
fn schema_validate_and_fork_match_the_oracle_golden() {
    let golden = golden();
    assert_eq!(golden["oracleVersion"], "3.0.0");
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), CASE_COUNT, "golden 的案例數");

    let mut fixture_cases: Vec<String> = std::fs::read_dir(fixture_root())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    fixture_cases.sort();
    let golden_cases: Vec<String> = cases
        .iter()
        .map(|case| case["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        fixture_cases, golden_cases,
        "fixture 目錄與 golden 的案例不一致"
    );

    let mut failures = Vec::new();
    let mut compared = 0;
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let runs = case["runs"].as_array().unwrap();
        assert_eq!(runs.len(), RUNS_PER_CASE, "{name}");
        for (index, run_spec) in runs.iter().enumerate() {
            let args: Vec<String> = run_spec["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str().unwrap().to_string())
                .collect();
            let oracle = Outcome {
                exit_code: run_spec["exitCode"].as_i64().unwrap(),
                stdout: run_spec["stdout"].as_str().unwrap().to_string(),
                stderr: run_spec["stderr"].as_str().unwrap().to_string(),
            };
            let root = project(name);
            let actual = run(&root, &args);
            compared += 1;
            match divergence(name, index) {
                Some(expected) => {
                    if expected == oracle {
                        failures.push(format!(
                            "{name} {args:?}: 列為分歧但與 oracle 相同，請刪掉這一列"
                        ));
                    }
                    if actual != expected {
                        failures.push(format!(
                            "{name} {args:?}: 刻意分歧的輸出不符\n  expected: {expected:?}\n  actual:   {actual:?}"
                        ));
                    }
                }
                None => {
                    if actual != oracle {
                        failures.push(format!(
                            "{name} {args:?}\n  oracle: {oracle:?}\n  actual: {actual:?}"
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(compared, CASE_COUNT * RUNS_PER_CASE);
    assert!(
        failures.is_empty(),
        "{} 處不符：\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// 內建 schema 的 fork 與 oracle 逐位元組相同（W7g 探測 p08：artifact 依
/// proposal、specs、design、tasks 的順序寫出），唯一的差異是第一行的 `name:`——
/// OpenSpectra 改寫成目標名稱，oracle 保留來源名稱（既有的刻意分歧）。
#[test]
fn builtin_forks_match_the_oracle_bytes_except_the_name_line() {
    let golden = golden();
    let forks = golden["builtinForks"].as_object().unwrap();
    assert_eq!(
        forks.keys().collect::<Vec<_>>(),
        vec!["no-spec", "spec-driven"]
    );
    for (source, fork) in forks {
        let root = TempDir::new(&format!("schema-fork-builtin-{source}"));
        std::fs::create_dir_all(root.join("openspec/changes/archive")).unwrap();
        std::fs::create_dir_all(root.join("openspec/specs")).unwrap();
        std::fs::write(root.join(".spectra.yaml"), "spec_dir: openspec\n").unwrap();
        std::fs::write(root.join("openspec/config.yaml"), "schema: spec-driven\n").unwrap();

        let actual = run(
            &root,
            &["schema".into(), "fork".into(), source.clone(), "f".into()],
        );
        assert_eq!(
            actual,
            ok(
                fork["stdout"].as_str().unwrap(),
                fork["stderr"].as_str().unwrap()
            ),
            "{source}"
        );

        let target = root.join("openspec/schemas/f");
        let mut written = Vec::new();
        collect_files(&target, &target, &mut written);
        written.sort();
        let files = fork["files"].as_object().unwrap();
        assert_eq!(
            written,
            files.keys().cloned().collect::<Vec<_>>(),
            "{source}: 寫出的檔案"
        );
        for (path, content) in files {
            let mut expected = content.as_str().unwrap().to_string();
            if path == "schema.yaml" {
                let oracle_name = format!("name: {source}\n");
                assert!(
                    expected.starts_with(&oracle_name),
                    "{source}: {expected:.40}"
                );
                expected = expected.replacen(&oracle_name, "name: f\n", 1);
            }
            assert_eq!(
                std::fs::read_to_string(target.join(path)).unwrap(),
                expected,
                "{source}: {path}"
            );
        }
    }
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(base, &path, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().display().to_string());
        }
    }
}
