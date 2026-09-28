//! 內建實作切換（docs/migration-plan.md「Phase 1」、D6 裁決）：決定一次 `spectra` 呼叫由
//! oracle（閉源 Spectra.app）還是 OpenSpectra 執行，以及 shadow 模式下的比對與紀錄。
//!
//! 這裡只放可測試的純邏輯（模式解析、唯讀白名單、輸出比對、log 行）；實際 exec／spawn
//! 在 CLI 的 `main`。
//!
//! 設定來源依序：環境變數 `OPENSPECTRA_IMPL` → 專案的 `.spectra/impl` → 使用者層級的
//! `$XDG_CONFIG_HOME/openspectra/impl`（預設 `~/.config/openspectra/impl`）→ `oss`。刻意不放進
//! `.spectra.yaml` 或 `spectra config` 的全域設定：oracle 也會讀那兩個檔。

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde_json::{json, Value};

pub const ENV_MODE: &str = "OPENSPECTRA_IMPL";
pub const ENV_ORACLE_BIN: &str = "OPENSPECTRA_ORACLE_BIN";
/// shadow 模式替 OpenSpectra 子行程設的旗標：子行程照常執行，但不寫 `errors.jsonl`。
pub const ENV_SHADOW_CHILD: &str = "OPENSPECTRA_SHADOW_CHILD";
pub const DEFAULT_ORACLE_BIN: &str = "/Applications/Spectra.app/Contents/MacOS/spectra";

/// shadow 模式可以同時跑兩個實作的**唯讀**子指令。會寫檔的指令（`archive`、`task`、`new`、
/// `park`、`update`、`init`……）絕對不能出現在這裡：兩個實作會對同一份檔案各寫一次。
pub const SHADOW_READONLY: &[&str] = &[
    "list",
    "show",
    "status",
    "validate",
    "analyze",
    "drift",
    "instructions",
    "schemas",
    "templates",
    "scope",
];

/// 已知會寫檔的子指令。只用於測試，確保它們不會被加進 [`SHADOW_READONLY`]。
pub const WRITING_SUBCOMMANDS: &[&str] = &[
    "init",
    "update",
    "archive",
    "new",
    "park",
    "unpark",
    "in-progress",
    "task",
    "config",
    "schema",
    "trace",
    "completion",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Oss,
    Oracle,
    Shadow,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Oss => "oss",
            Mode::Oracle => "oracle",
            Mode::Shadow => "shadow",
        }
    }
}

/// 模式的設定來源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Env,
    Project(PathBuf),
    User(PathBuf),
    Default,
}

impl Source {
    pub fn describe(&self) -> String {
        match self {
            Source::Env => ENV_MODE.to_string(),
            Source::Project(p) | Source::User(p) => p.display().to_string(),
            Source::Default => "default".to_string(),
        }
    }
}

pub fn parse_mode(raw: &str, origin: &str) -> Result<Mode> {
    match raw.trim() {
        "oss" => Ok(Mode::Oss),
        "oracle" => Ok(Mode::Oracle),
        "shadow" => Ok(Mode::Shadow),
        other => bail!(
            "invalid implementation mode '{other}' in {origin} (expected oracle, shadow, or oss)"
        ),
    }
}

/// 依優先序決定模式。`project`／`user` 是（檔案路徑, 內容）。空字串的環境變數視為未設定。
pub fn resolve(
    env: Option<&str>,
    project: Option<(PathBuf, String)>,
    user: Option<(PathBuf, String)>,
) -> Result<(Mode, Source)> {
    if let Some(value) = env.filter(|v| !v.trim().is_empty()) {
        return Ok((parse_mode(value, ENV_MODE)?, Source::Env));
    }
    if let Some((path, text)) = project {
        return Ok((
            parse_mode(&text, &path.display().to_string())?,
            Source::Project(path),
        ));
    }
    if let Some((path, text)) = user {
        return Ok((
            parse_mode(&text, &path.display().to_string())?,
            Source::User(path),
        ));
    }
    Ok((Mode::Oss, Source::Default))
}

/// 從 `cwd` 往上找專案根（有 `.spectra.yaml` 的目錄）底下的 `.spectra/impl`。
pub fn project_impl_file(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|dir| crate::config::Config::is_project_root(dir))
        .map(|root| root.join(".spectra").join("impl"))
        .filter(|p| p.is_file())
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// 使用者層級設定檔：`$XDG_CONFIG_HOME/openspectra/impl`，預設 `~/.config/openspectra/impl`。
pub fn user_impl_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home().map(|h| h.join(".config")))?;
    Some(base.join("openspectra").join("impl"))
}

/// log 目錄：`$XDG_STATE_HOME/openspectra`，預設 `~/.local/state/openspectra`。
pub fn state_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home().map(|h| h.join(".local").join("state")))?;
    Some(base.join("openspectra"))
}

/// log 用的時間戳（UTC，RFC 3339，秒精度）。
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn oracle_bin(env: Option<&str>) -> PathBuf {
    PathBuf::from(env.filter(|v| !v.is_empty()).unwrap_or(DEFAULT_ORACLE_BIN))
}

/// argv（不含程式名）的子指令：跳過開頭的全域旗標（`--no-color`）。
pub fn subcommand(args: &[String]) -> Option<&str> {
    args.iter()
        .map(String::as_str)
        .find(|a| !a.starts_with('-'))
}

/// shadow 模式能否同時執行兩個實作。`--help`／`--version` 等沒有子指令的呼叫不比對。
pub fn is_shadowable(args: &[String]) -> bool {
    subcommand(args).is_some_and(|s| SHADOW_READONLY.contains(&s))
}

/// 一次執行的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// 兩邊輸出的差異摘要；相同時為空。stdout 兩邊都是 JSON 時做語意比對（物件 key 順序不計，
/// 陣列順序計），列出最多 `limit` 個不同的路徑；否則逐位元組比對。
pub fn compare(oracle: &Captured, oss: &Captured, limit: usize) -> Vec<String> {
    let mut diffs = Vec::new();
    if oracle.code != oss.code {
        diffs.push(format!("exit {}->{}", oracle.code, oss.code));
    }
    let parsed = (
        serde_json::from_slice::<Value>(&oracle.stdout),
        serde_json::from_slice::<Value>(&oss.stdout),
    );
    match parsed {
        (Ok(a), Ok(b)) => {
            let mut paths = Vec::new();
            json_diff(&a, &b, String::new(), &mut paths, limit);
            diffs.extend(paths.into_iter().map(|p| format!("stdout {p}")));
        }
        _ if oracle.stdout != oss.stdout => diffs.push("stdout text-differs".to_string()),
        _ => {}
    }
    if oracle.stderr != oss.stderr {
        diffs.push("stderr differs".to_string());
    }
    diffs
}

fn json_diff(a: &Value, b: &Value, path: String, out: &mut Vec<String>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let p = format!("{path}.{k}");
                match (x.get(k), y.get(k)) {
                    (Some(va), Some(vb)) => json_diff(va, vb, p, out, limit),
                    (Some(_), None) => out.push(format!("{p} oracle-only")),
                    (None, _) => out.push(format!("{p} oss-only")),
                }
                if out.len() >= limit {
                    return;
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("{path}[] len {}->{}", x.len(), y.len()));
                return;
            }
            for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                json_diff(va, vb, format!("{path}[{i}]"), out, limit);
                if out.len() >= limit {
                    return;
                }
            }
        }
        _ if a != b => out.push(format!(
            "{} value",
            if path.is_empty() { "." } else { &path }
        )),
        _ => {}
    }
}

/// `shadow.jsonl` 的一行。
pub fn shadow_record(
    ts: &str,
    cwd: &Path,
    args: &[String],
    oracle: &Captured,
    oss: &Captured,
    diffs: &[String],
) -> String {
    json!({
        "ts": ts,
        "cwd": cwd.to_string_lossy(),
        "argv": args,
        "oracle_exit": oracle.code,
        "oss_exit": oss.code,
        "diffs": diffs,
    })
    .to_string()
}

/// `errors.jsonl` 的一行（`oss` 模式的非 0 exit 或 panic）。
pub fn error_record(
    ts: &str,
    cwd: &Path,
    args: &[String],
    code: i32,
    summary: &str,
    panic: bool,
) -> String {
    json!({
        "ts": ts,
        "cwd": cwd.to_string_lossy(),
        "argv": args,
        "exit": code,
        "summary": summary,
        "panic": panic,
    })
    .to_string()
}

/// 在 `dir/file` 附加一行（建立目錄）。
pub fn append_line(dir: &Path, file: &str, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(file))?;
    writeln!(f, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolve_follows_env_then_project_then_user_then_default() {
        let project = Some((PathBuf::from("/p/.spectra/impl"), "shadow\n".to_string()));
        let user = Some((PathBuf::from("/u/impl"), "oracle".to_string()));
        assert_eq!(
            resolve(Some("oss"), project.clone(), user.clone()).unwrap(),
            (Mode::Oss, Source::Env)
        );
        assert_eq!(
            resolve(None, project.clone(), user.clone()).unwrap(),
            (
                Mode::Shadow,
                Source::Project(PathBuf::from("/p/.spectra/impl"))
            )
        );
        assert_eq!(
            resolve(Some("  "), None, user.clone()).unwrap(),
            (Mode::Oracle, Source::User(PathBuf::from("/u/impl")))
        );
        assert_eq!(
            resolve(None, None, None).unwrap(),
            (Mode::Oss, Source::Default)
        );
    }

    #[test]
    fn invalid_mode_is_an_error_naming_its_origin() {
        let err = resolve(Some("fast"), None, None).unwrap_err().to_string();
        assert_eq!(
            err,
            "invalid implementation mode 'fast' in OPENSPECTRA_IMPL (expected oracle, shadow, or oss)"
        );
        let err = resolve(
            None,
            Some((PathBuf::from("/p/impl"), "ORACLE".into())),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("in /p/impl"), "{err}");
    }

    /// 會寫檔的指令絕對不能被 shadow（兩個實作會各寫一次）。
    #[test]
    fn writing_subcommands_are_never_shadowed() {
        for cmd in WRITING_SUBCOMMANDS {
            assert!(!SHADOW_READONLY.contains(cmd), "{cmd} must not be shadowed");
            assert!(!is_shadowable(&args(&[cmd, "x"])), "{cmd}");
        }
        assert!(is_shadowable(&args(&["--no-color", "list", "--json"])));
        assert!(!is_shadowable(&args(&["--version"])));
        assert!(!is_shadowable(&args(&[])));
    }

    fn cap(code: i32, stdout: &str, stderr: &str) -> Captured {
        Captured {
            code,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn compare_is_semantic_for_json_and_exact_for_text() {
        let a = cap(0, r#"{"b": 1, "a": [1, 2]}"#, "");
        let b = cap(0, "{\n  \"a\": [1, 2],\n  \"b\": 1\n}\n", "");
        assert!(compare(&a, &b, 10).is_empty());

        let c = cap(1, r#"{"a": [1, 3], "c": true}"#, "warn\n");
        assert_eq!(
            compare(&a, &c, 10),
            [
                "exit 0->1",
                "stdout .a[1] value",
                "stdout .b oracle-only",
                "stdout .c oss-only",
                "stderr differs"
            ]
        );
        assert_eq!(
            compare(&cap(0, "x\n", ""), &cap(0, "y\n", ""), 10),
            ["stdout text-differs"]
        );
        // limit 只限 stdout 的路徑數：exit + 2 個路徑 + stderr。
        assert_eq!(compare(&a, &c, 2).len(), 4, "limit caps the stdout paths");
    }

    #[test]
    fn project_impl_file_is_found_from_a_subdirectory() {
        let tmp = crate::test_support::TempDir::new("impl-switch-project");
        std::fs::write(tmp.join(".spectra.yaml"), "spec_dir: openspec\n").unwrap();
        std::fs::create_dir_all(tmp.join(".spectra")).unwrap();
        std::fs::create_dir_all(tmp.join("a/b")).unwrap();
        assert_eq!(project_impl_file(&tmp.join("a/b")), None);
        std::fs::write(tmp.join(".spectra/impl"), "shadow\n").unwrap();
        assert_eq!(
            project_impl_file(&tmp.join("a/b")),
            Some(tmp.join(".spectra/impl"))
        );
    }
}
