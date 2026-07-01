//! OpenSpectra CLI: `drift`, `list`, `show`.

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
    /// (not yet implemented) Disable colored output.
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
    /// List active changes (or specs with --specs).
    List {
        /// (not yet implemented) Filter to changes only.
        #[arg(long)]
        changes: bool,
        /// List specs instead of changes.
        #[arg(long)]
        specs: bool,
        /// (not yet implemented) List parked changes.
        #[arg(long)]
        parked: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show a change's proposal. (spec content: not yet implemented)
    Show {
        /// Change name to show.
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

fn cmd_drift(cfg: &Config, change_name: Option<&str>, as_json: bool) -> Result<i32> {
    let name = change::resolve(cfg, change_name)?;
    let change = change::load(cfg, &name)?;
    let report = drift::analyze(cfg, &change)?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report);
    }
    Ok(report.exit_code())
}

/// Conclusion-first human report (mirrors the reference layout: plain-language
/// next step, then a scorecard, then non-empty technical detail).
fn print_human(r: &drift::DriftReport) {
    println!("## Drift Report: {}\n", r.change_id);

    let conclusion = match r.severity.as_str() {
        "light" => "Drift is minor — you can start work directly.",
        "medium" => "The change has drifted moderately; refresh the plan before implementing.",
        _ => "The change has drifted heavily; the old plan likely no longer fits — archive or restart.",
    };
    println!("{conclusion}\n");

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

fn cmd_list(cfg: &Config, want_specs: bool, want_parked: bool, as_json: bool) -> Result<i32> {
    // Specs have no task/parked state of their own, so --specs takes priority
    // over --parked rather than trying to combine the two filters.
    if want_specs {
        return cmd_list_specs(cfg, as_json);
    }
    // Minimal: active changes with task counts and a one-line summary.
    let names = if want_parked {
        Vec::new() // parked listing not yet implemented
    } else {
        change::list_active(cfg)
    };
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
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "changes": items }))?
        );
    } else if items.is_empty() {
        println!("No active changes.");
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
        let sp = spec::load(cfg, name)?;
        let summary = first_line(&sp.spec_md());
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

fn cmd_show(cfg: &Config, item: &str, as_json: bool) -> Result<i32> {
    let ch = change::load(cfg, item)?;
    let proposal = std::fs::read_to_string(ch.proposal_md()).unwrap_or_default();
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "name": item,
                "proposal": proposal,
            }))?
        );
    } else {
        print!("{proposal}");
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
    if cli.no_color {
        // Reserved: colored output is not yet emitted, so this is a no-op today.
    }
    let cwd = std::env::current_dir().context("getting current directory")?;
    let root = find_root(&cwd);

    match &cli.command {
        Command::Drift { change, json } => {
            let cfg = require_initialized(&root)?;
            cmd_drift(&cfg, change.as_deref(), *json)
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
        assert!(wrapped.get("specs").is_some());
        let round_tripped: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&wrapped).unwrap()).unwrap();
        assert_eq!(round_tripped["specs"][0]["name"], "auth");
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
}
