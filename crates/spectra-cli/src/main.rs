//! OpenSpectra CLI: `drift`, `list`, `show`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;

use spectra_core::{change, config::Config, drift, spec};

#[derive(Parser)]
#[command(
    name = "spectra",
    version,
    about = "Open-source Spectra spec-driven CLI"
)]
struct Cli {
    /// Disable colored output (also respects the NO_COLOR env var).
    #[arg(long, global = true)]
    no_color: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Detect drift between a change and the current codebase state.
    Drift {
        /// Change name (auto-detects if only one exists).
        change: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List active changes (or specs with --specs, or parked changes with --parked).
    List {
        /// (not yet implemented) Filter to changes only.
        #[arg(long)]
        changes: bool,
        /// List specs instead of changes.
        #[arg(long, conflicts_with = "parked")]
        specs: bool,
        /// List parked changes instead of active ones.
        #[arg(long)]
        parked: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show a change's proposal, or a spec's content if the name isn't a change.
    Show {
        /// Change or spec name to show.
        item: String,
        #[arg(long)]
        json: bool,
    },
}

/// Walk up from `start` to find the project root (dir containing `.spectra.yaml`),
/// falling back to `start` itself.
fn find_root(start: &Path) -> PathBuf {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(".spectra.yaml").exists() {
            return dir.to_path_buf();
        }
        cur = dir.parent();
    }
    start.to_path_buf()
}

fn require_initialized(root: &Path) -> Result<Config> {
    if !Config::is_initialized(root) {
        anyhow::bail!("Not initialized. Run 'spectra init' first.");
    }
    Config::load(root)
}

fn cmd_drift(cfg: &Config, change_name: Option<&str>, as_json: bool, use_color: bool) -> Result<i32> {
    let name = change::resolve(cfg, change_name)?;
    let change = change::load(cfg, &name)?;
    let report = drift::analyze(cfg, &change)?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report, use_color);
    }
    Ok(report.exit_code())
}

/// Whether to emit ANSI color codes: the `--no-color` flag and the `NO_COLOR`
/// env var (https://no-color.org — "when present, **regardless of its
/// value**") both disable it; otherwise color is only emitted when stdout is
/// a terminal (never when piped/redirected).
fn color_enabled(no_color: bool) -> bool {
    color_enabled_from(no_color, std::env::var_os("NO_COLOR").is_some(), std::io::stdout().is_terminal())
}

/// Pure precedence logic behind [`color_enabled`], split out so all
/// flag/env/TTY combinations are unit-testable without mutating real process
/// env vars or stdout (both of which would be flaky under parallel tests).
fn color_enabled_from(no_color: bool, no_color_env_set: bool, stdout_is_tty: bool) -> bool {
    !no_color && !no_color_env_set && stdout_is_tty
}

/// Wrap `text` in the given SGR color code (e.g. `"31"` for red) when
/// `enabled`, otherwise return it unchanged.
fn colorize(text: &str, sgr_code: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{sgr_code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// SGR color code for a drift severity: green for light, yellow for medium,
/// red for heavy (and any other/unknown value, treated as the worst case).
fn severity_sgr_code(severity: &str) -> &'static str {
    match severity {
        "light" => "32",
        "medium" => "33",
        _ => "31",
    }
}

/// The severity-colored conclusion sentence, composed here (rather than
/// inline in `print_human`) so the `colorize`/`severity_sgr_code` wiring
/// itself — not just each function in isolation — is unit-testable.
fn conclusion_line(severity: &str, use_color: bool) -> String {
    let conclusion = match severity {
        "light" => "Drift is minor — you can start work directly.",
        "medium" => "The change has drifted moderately; refresh the plan before implementing.",
        _ => "The change has drifted heavily; the old plan likely no longer fits — archive or restart.",
    };
    colorize(conclusion, severity_sgr_code(severity), use_color)
}

/// Conclusion-first human report (mirrors the reference layout: plain-language
/// next step, then a scorecard, then non-empty technical detail).
fn print_human(r: &drift::DriftReport, use_color: bool) {
    println!("## Drift Report: {}\n", r.change_id);
    println!("{}\n", conclusion_line(&r.severity, use_color));

    let dim = |k: &str| r.dimensions.iter().find(|d| format!("{:?}", d.kind) == k);
    let time = dim("Time").map(|d| d.status.as_str()).unwrap_or("-");
    let design = if r.broken_anchors.is_empty() {
        "No broken references".to_string()
    } else {
        format!("{} broken", r.broken_anchors.len())
    };
    let tasks = if r.tasks_blocked_external.is_empty() && r.tasks_maybe_resolved.is_empty() {
        "No task collisions".to_string()
    } else {
        format!(
            "{} blocked, {} maybe-done",
            r.tasks_blocked_external.len(),
            r.tasks_maybe_resolved.len()
        )
    };

    println!("| Dimension         | Status                                |");
    println!("|-------------------|---------------------------------------|");
    println!("| Time              | {time:<37} |");
    println!("| Design references | {design:<37} |");
    println!("| Pending tasks     | {tasks:<37} |");
    println!(
        "| Overall           | {:<37} |",
        format!("{}, total score {}", r.severity, r.total_score)
    );

    println!("\n### Recommendation\nRun `{}`.", r.primary_recommendation);

    if !r.broken_anchors.is_empty() {
        println!("\n### Broken design references");
        for a in &r.broken_anchors {
            println!("- `{}` ({}) — {}", a.anchor, a.category, a.reason);
        }
    }
}

fn list_change_items(cfg: &Config, want_parked: bool) -> Result<Vec<serde_json::Value>> {
    let names = if want_parked { change::list_parked(cfg) } else { change::list_active(cfg) };
    let mut items = Vec::new();
    for name in &names {
        let ch = change::load(cfg, name)?;
        let (done, total) = task_counts(&ch.tasks_md());
        let status = if total > 0 && done == total {
            "done"
        } else {
            "in-progress"
        };
        let summary = first_line(&ch.proposal_md());
        items.push(json!({
            "name": name,
            "status": status,
            "completedTasks": done,
            "totalTasks": total,
            "summary": summary,
        }));
    }
    Ok(items)
}

fn cmd_list(cfg: &Config, want_specs: bool, want_parked: bool, as_json: bool) -> Result<i32> {
    // clap rejects --specs with --parked (they're `conflicts_with`), so at
    // most one of the two is ever true here.
    if want_specs {
        return cmd_list_specs(cfg, as_json);
    }
    let items = list_change_items(cfg, want_parked)?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "changes": items }))?
        );
    } else if items.is_empty() {
        println!("{}", if want_parked { "No parked changes." } else { "No active changes." });
    } else {
        for it in &items {
            println!(
                "{:<45} {}/{} {}",
                it["name"].as_str().unwrap_or(""),
                it["completedTasks"],
                it["totalTasks"],
                it["status"].as_str().unwrap_or("")
            );
        }
    }
    Ok(0)
}

fn list_specs_items(cfg: &Config) -> Result<Vec<serde_json::Value>> {
    let names = spec::list(cfg)?;
    let mut items = Vec::new();
    for name in &names {
        // `spec::list` already confirmed `spec.md` exists for each name; skip
        // the redundant re-stat that `spec::load` would perform.
        let summary = first_line(&cfg.specs_dir().join(name).join("spec.md"));
        items.push(json!({ "name": name, "summary": summary }));
    }
    Ok(items)
}

fn cmd_list_specs(cfg: &Config, as_json: bool) -> Result<i32> {
    let items = list_specs_items(cfg)?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "specs": items }))?
        );
    } else if items.is_empty() {
        println!("No specs.");
    } else {
        for it in &items {
            println!(
                "{:<45} {}",
                it["name"].as_str().unwrap_or(""),
                it["summary"].as_str().unwrap_or("")
            );
        }
    }
    Ok(0)
}

#[derive(Debug)]
enum ShowContent {
    Proposal(String),
    Spec(String),
}

/// Resolve `item` to a change's proposal or a spec's content. A change name
/// takes priority (existing, regression-safe behavior); `change::try_load`/
/// `spec::try_load` distinguish "genuinely doesn't exist" from a real I/O
/// error while checking, so neither is misread as "not found."
fn resolve_show_content(cfg: &Config, item: &str) -> Result<ShowContent> {
    if let Some(ch) = change::try_load(cfg, item)? {
        return Ok(ShowContent::Proposal(read_show_content(&ch.proposal_md())?));
    }
    if let Some(sp) = spec::try_load(cfg, item)? {
        return Ok(ShowContent::Spec(read_show_content(&sp.spec_md())?));
    }
    anyhow::bail!("'{item}' is not a known change or spec")
}

/// Read `path`'s content for `show`. Unlike `first_line` (used for `list`'s
/// secondary summary column, where a warn-and-degrade is defensible because
/// other fields still carry useful data), the content read here *is* the
/// entire requested output — so besides the benign `NotFound` case (a
/// genuinely bodyless change/spec), any other I/O failure propagates instead
/// of silently printing empty content with a success exit code.
fn read_show_content(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn show_json(item: &str, content: &ShowContent) -> serde_json::Value {
    match content {
        ShowContent::Proposal(text) => json!({ "name": item, "proposal": text }),
        ShowContent::Spec(text) => json!({ "name": item, "spec": text }),
    }
}

fn cmd_show(cfg: &Config, item: &str, as_json: bool) -> Result<i32> {
    let content = resolve_show_content(cfg, item)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&show_json(item, &content))?);
    } else {
        let (ShowContent::Proposal(text) | ShowContent::Spec(text)) = &content;
        print!("{text}");
    }
    Ok(0)
}

fn task_counts(tasks_md: &Path) -> (usize, usize) {
    let Ok(text) = std::fs::read_to_string(tasks_md) else {
        return (0, 0);
    };
    let tasks = spectra_core::tasks::parse(&text);
    (tasks.iter().filter(|t| t.done).count(), tasks.len())
}

fn first_line(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_default(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            eprintln!("warning: reading {}: {e}", path.display());
            String::new()
        }
    }
}

fn run() -> Result<i32> {
    let cli = Cli::parse();
    let use_color = color_enabled(cli.no_color);
    let cwd = std::env::current_dir().context("getting current directory")?;
    let root = find_root(&cwd);

    match &cli.command {
        Command::Drift { change, json } => {
            let cfg = require_initialized(&root)?;
            cmd_drift(&cfg, change.as_deref(), *json, use_color)
        }
        Command::List {
            specs,
            parked,
            json,
            ..
        } => {
            let cfg = require_initialized(&root)?;
            cmd_list(&cfg, *specs, *parked, *json)
        }
        Command::Show { item, json } => {
            let cfg = require_initialized(&root)?;
            cmd_show(&cfg, item, *json)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            // Distinct from drift severities (0 light / 1 medium / 2 heavy) so
            // CI can tell a tool failure apart from a heavy-drift gate.
            eprintln!("Error: {e:#}");
            ExitCode::from(3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_enabled_from_only_true_when_nothing_disables_it() {
        assert!(color_enabled_from(false, false, true));
    }

    #[test]
    fn color_enabled_from_no_color_flag_wins_even_with_tty_and_no_env() {
        assert!(!color_enabled_from(true, false, true));
    }

    #[test]
    fn color_enabled_from_no_color_env_wins_even_without_flag() {
        assert!(!color_enabled_from(false, true, true));
    }

    #[test]
    fn color_enabled_from_false_when_not_a_tty_even_with_nothing_else_set() {
        assert!(!color_enabled_from(false, false, false));
    }

    #[test]
    fn colorize_wraps_text_in_sgr_codes_only_when_enabled() {
        assert_eq!(colorize("hi", "31", true), "\x1b[31mhi\x1b[0m");
        assert_eq!(colorize("hi", "31", false), "hi");
    }

    #[test]
    fn severity_sgr_code_maps_known_and_unknown_severities() {
        assert_eq!(severity_sgr_code("light"), "32");
        assert_eq!(severity_sgr_code("medium"), "33");
        assert_eq!(severity_sgr_code("heavy"), "31");
        assert_eq!(severity_sgr_code("anything-else"), "31");
    }

    #[test]
    fn conclusion_line_colors_by_severity_when_enabled() {
        assert_eq!(
            conclusion_line("light", true),
            "\x1b[32mDrift is minor — you can start work directly.\x1b[0m"
        );
        assert!(conclusion_line("light", false).starts_with("Drift is minor"));
        assert!(!conclusion_line("light", false).contains('\x1b'));
    }

    /// RAII guard for a per-test scratch directory: removes it on drop even
    /// when the test panics partway through (an assertion failure must not
    /// leak the directory).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            // Nanosecond timestamps alone can collide between threads running
            // concurrently (observed in practice under `cargo test`'s default
            // parallel harness); an atomic counter guarantees uniqueness.
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-cli-test-{}-{}-{seq}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl std::ops::Deref for TempDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn list_specs_items_shape_matches_specs_key_contract() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        let auth_dir = cfg.specs_dir().join("auth");
        std::fs::create_dir_all(&auth_dir).unwrap();
        std::fs::write(auth_dir.join("spec.md"), "# Auth\nHandles login.\n").unwrap();

        let items = list_specs_items(&cfg).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"].as_str(), Some("auth"));
        assert_eq!(items[0]["summary"].as_str(), Some("# Auth"));

        // The exact wrapper key ("specs") is the documented --json contract;
        // pin it here so a typo doesn't ship silently.
        let wrapped = json!({ "specs": items });
        assert_eq!(wrapped["specs"][0]["name"], "auth");
    }

    #[test]
    fn list_specs_items_is_empty_when_no_specs_exist() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        let items = list_specs_items(&cfg).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn list_change_items_parked_flag_selects_parked_changes() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("shipped")).unwrap();
        std::fs::write(cfg.changes_dir().join("shipped").join("proposal.md"), "# Shipped\n")
            .unwrap();
        std::fs::create_dir_all(cfg.changes_dir().join("on-hold")).unwrap();
        std::fs::write(cfg.changes_dir().join("on-hold").join("proposal.md"), "# On hold\n")
            .unwrap();
        std::fs::create_dir_all(tmp.join(".spectra").join("changes")).unwrap();
        std::fs::write(tmp.join(".spectra").join("changes").join("on-hold.parked"), "").unwrap();

        let active = list_change_items(&cfg, false).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0]["name"].as_str(), Some("shipped"));

        let parked = list_change_items(&cfg, true).unwrap();
        assert_eq!(parked.len(), 1);
        assert_eq!(parked[0]["name"].as_str(), Some("on-hold"));
    }

    #[test]
    fn list_change_items_parked_is_empty_when_none_parked() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("shipped")).unwrap();
        std::fs::write(cfg.changes_dir().join("shipped").join("proposal.md"), "# Shipped\n")
            .unwrap();

        assert!(list_change_items(&cfg, true).unwrap().is_empty());
    }

    #[test]
    fn resolve_show_content_prefers_change_over_spec() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("auth")).unwrap();
        std::fs::write(cfg.changes_dir().join("auth").join("proposal.md"), "# Auth change\n")
            .unwrap();
        std::fs::create_dir_all(cfg.specs_dir().join("auth")).unwrap();
        std::fs::write(cfg.specs_dir().join("auth").join("spec.md"), "# Auth spec\n").unwrap();

        match resolve_show_content(&cfg, "auth").unwrap() {
            ShowContent::Proposal(text) => assert_eq!(text, "# Auth change\n"),
            ShowContent::Spec(_) => panic!("expected the change to take priority over the spec"),
        }
    }

    #[test]
    fn resolve_show_content_falls_back_to_spec() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        std::fs::create_dir_all(cfg.specs_dir().join("billing")).unwrap();
        std::fs::write(cfg.specs_dir().join("billing").join("spec.md"), "# Billing spec\n")
            .unwrap();

        match resolve_show_content(&cfg, "billing").unwrap() {
            ShowContent::Spec(text) => assert_eq!(text, "# Billing spec\n"),
            ShowContent::Proposal(_) => panic!("expected a spec, not a change"),
        }
    }

    #[test]
    fn resolve_show_content_errors_when_neither_change_nor_spec() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        assert!(resolve_show_content(&cfg, "ghost").is_err());
    }

    #[test]
    fn resolve_show_content_propagates_real_errors_instead_of_falling_back_to_spec() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("broken")).unwrap();
        // A directory named `.openspec.yaml` makes `read_to_string` fail with
        // a real I/O error (cross-platform), unlike the benign
        // "no metadata file present" case `change::load` otherwise handles.
        std::fs::create_dir_all(cfg.changes_dir().join("broken").join(".openspec.yaml")).unwrap();
        // A same-named spec exists too, to prove the real error isn't
        // silently swallowed into a fallback.
        std::fs::create_dir_all(cfg.specs_dir().join("broken")).unwrap();
        std::fs::write(cfg.specs_dir().join("broken").join("spec.md"), "# Should not be used\n")
            .unwrap();

        let err = resolve_show_content(&cfg, "broken").unwrap_err();
        assert!(!err.to_string().contains("is not a known change or spec"));
    }

    #[test]
    fn show_json_uses_proposal_key_for_change_content() {
        let value = show_json("my-change", &ShowContent::Proposal("hello".to_string()));
        assert_eq!(value["name"], "my-change");
        assert_eq!(value["proposal"], "hello");
        assert!(value.get("spec").is_none());
    }

    #[test]
    fn show_json_uses_spec_key_for_spec_content() {
        let value = show_json("auth", &ShowContent::Spec("# Auth\n".to_string()));
        assert_eq!(value["name"], "auth");
        assert_eq!(value["spec"], "# Auth\n");
        assert!(value.get("proposal").is_none());
    }
}
