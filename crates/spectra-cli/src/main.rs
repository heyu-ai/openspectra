//! OpenSpectra CLI: `init`, `drift`, `analyze`, `schemas`, `completion`,
//! `status`, `instructions`, `validate`, `list`, `show`, `park`, `unpark`,
//! `in-progress add`, `new change`, `new artifact`, `task done`, `archive`,
//! `update`, `config`.

mod completion;

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use serde_json::json;

use spectra_core::{analyze, artifact, change, config::Config, drift, instructions, schema, spec};

#[derive(Parser, Debug)]
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

#[derive(Subcommand, Debug)]
enum Command {
    /// Scaffold a fresh project: `.spectra.yaml`, `<spec_dir>/{changes,specs}/`,
    /// and a `.spectra/` entry in `.gitignore`. Every other command requires
    /// this to have run first.
    Init {
        #[arg(long)]
        adopt: bool,
        #[arg(long)]
        json: bool,
    },
    /// Detect drift between a change and the current codebase state.
    Drift {
        /// Change name (auto-detects if only one exists).
        change: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Analyze change artifacts for consistency and gaps.
    Analyze {
        #[arg(help = "Change name (auto-detects if only one exists)")]
        change: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Generate, install, or uninstall shell completions.
    Completion {
        #[command(subcommand)]
        target: CompletionTarget,
    },
    /// List available workflow schemas (only `spec-driven` is built in).
    Schemas {
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show workflow artifact status for a change.
    Status {
        /// Change name (auto-detects if only one exists).
        #[arg(long)]
        change: Option<String>,
        /// Workflow schema name (only `spec-driven` is built in).
        #[arg(long)]
        schema: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Get instructions for an artifact.
    Instructions {
        /// Artifact ID (proposal|design|specs|tasks) or "apply".
        artifact: Option<String>,
        /// Change name (auto-detects if only one exists).
        #[arg(long)]
        change: Option<String>,
        /// Workflow schema name (only `spec-driven` is built in).
        #[arg(long)]
        schema: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
        /// Embedded skill name (not supported in openspectra).
        #[arg(long)]
        skill: Option<String>,
    },
    /// Validate changes against the OpenSpec structural rules (a change needs
    /// at least one requirement delta; with --strict, each ADDED/MODIFIED
    /// requirement also needs a normative SHALL/MUST and a `#### Scenario:`).
    /// Unlike `drift`, this is a pass/fail gate: it exits non-zero when any
    /// change is invalid.
    Validate {
        /// Change name to validate (auto-detects if only one active change
        /// exists). Ignored when --changes is given.
        change: Option<String>,
        /// Validate every active change instead of a single one.
        #[arg(long, conflicts_with = "change")]
        changes: bool,
        /// Escalate content-quality findings (missing SHALL/MUST or missing
        /// scenario) from ignored to hard errors.
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        json: bool,
    },
    /// List active changes (or specs with --specs, or parked changes with --parked).
    List {
        /// List active changes explicitly (the default when no filter flag
        /// is given; mutually exclusive with --specs/--parked).
        #[arg(long, conflicts_with_all = ["specs", "parked"])]
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
    /// Park a change (mark it on hold, excluding it from the active listing).
    Park {
        /// Change name to park.
        change: String,
        #[arg(long)]
        json: bool,
    },
    /// Unpark a change (resume it from a parked state).
    Unpark {
        /// Change name to unpark.
        change: String,
        #[arg(long)]
        json: bool,
    },
    /// In-progress marker operations.
    InProgress {
        #[command(subcommand)]
        target: InProgressTarget,
    },
    /// Create a new change or workflow artifact.
    New {
        #[command(subcommand)]
        target: NewTarget,
    },
    /// Task operations (currently the only `task` target).
    Task {
        #[command(subcommand)]
        target: TaskTarget,
    },
    /// Update instruction files
    Update {
        /// Project path (defaults to current directory)
        path: Option<PathBuf>,
        /// Overwrite existing files
        #[arg(long)]
        force: bool,
    },
    /// Archive a completed change (move it to `changes/archive/<date>-<name>`
    /// and apply its added spec requirements, unless --skip-specs).
    Archive {
        /// Change to archive (auto-detects if only one active change exists).
        change: Option<String>,
        /// Skip applying the change's spec deltas to the canonical specs.
        #[arg(long)]
        skip_specs: bool,
        /// Mark all incomplete tasks as complete before archiving.
        #[arg(long)]
        mark_tasks_complete: bool,
    },
    /// Config management commands
    Config {
        #[command(subcommand)]
        target: ConfigTarget,
    },
}

/// Subcommands of `spectra config`, managing the *global* user config file
/// (see `spectra_core::global_config`) — none of them need an initialized
/// project (probed: the oracle runs them outside any project).
#[derive(Subcommand, Debug)]
enum ConfigTarget {
    /// Show config file path
    Path,
    /// List all settings
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a config value
    Get {
        /// Config key
        key: String,
    },
    /// Set a config value
    Set {
        /// Config key
        key: String,
        /// Config value
        value: String,
        /// Treat value as string
        #[arg(long)]
        string: bool,
        /// Allow unknown keys
        // Accepted but inert, mirroring the oracle (probed: 2.3.1 accepts
        // unknown keys with or without this flag).
        #[arg(long)]
        allow_unknown: bool,
    },
    /// Remove a config key
    Unset {
        /// Config key
        key: String,
    },
    /// Reset config
    Reset {
        /// Reset all settings
        // NOT inert (probed): plain `reset` truncates the file to `{}`, while
        // `--all` deletes it outright. See `cmd_config`'s Reset arm.
        #[arg(long)]
        all: bool,
        /// Skip confirmation
        // Inert (probed): neither reset mode ever prompts, on a TTY or piped.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Edit config in $EDITOR
    Edit,
}

#[derive(Subcommand, Debug)]
enum NewTarget {
    /// Create a change directory with .openspec.yaml and, when run inside a
    /// git repo with at least one commit, a baseline git SHA. Artifact files
    /// are created later by the workflow.
    Change {
        /// Name for the new change (kebab-case, e.g. 'add-search-filter').
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a workflow artifact for a change.
    Artifact {
        #[arg(
            value_name = "TYPE",
            help = "Artifact type: proposal, design, tasks, spec"
        )]
        type_name: String,
        #[arg(help = "Capability name (required for spec type)")]
        capability: Option<String>,
        #[arg(long, help = "Change name")]
        change: Option<String>,
        #[arg(long, help = "Read content from stdin instead of using empty template")]
        stdin: bool,
        #[arg(long, help = "Overwrite existing artifact")]
        force: bool,
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum TaskTarget {
    /// Mark a task as done and record touched files.
    Done {
        /// Task ID (1-based sequential index across all tasks.md checkboxes).
        task_id: String,
        /// Change name (auto-detects if only one active change exists).
        #[arg(long)]
        change: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum CompletionTarget {
    /// Generate a completion script to stdout; the shell auto-detects from
    /// the SHELL environment variable when omitted.
    Generate { shell: Option<clap_complete::Shell> },
    /// Install a completion script in the shell's user completion directory.
    Install {
        shell: Option<clap_complete::Shell>,
        #[arg(long)]
        verbose: bool,
    },
    /// Uninstall the shell's completion script.
    Uninstall {
        shell: Option<clap_complete::Shell>,
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum InProgressTarget {
    /// Record an in-progress marker for a change name.
    Add { name: String },
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

/// `--json` shape for `init`: pinned here (rather than inlined in
/// `cmd_init`), matching `show_json`/`park_status_json`/`new_change_json`.
fn init_json(outcome: &spectra_core::init::InitOutcome) -> serde_json::Value {
    json!({
        "root": outcome.root.to_string_lossy(),
        "spec_dir": outcome.spec_dir,
        "adopted": outcome.adopted,
        "gitignore_updated": outcome.gitignore_updated,
    })
}

fn cmd_init(root: &Path, adopt: bool, as_json: bool) -> Result<i32> {
    let outcome = spectra_core::init::init_with_options(root, adopt)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&init_json(&outcome))?);
    } else if outcome.adopted {
        println!(
            "Adopted existing spectra project in {} (spec_dir: {}).",
            outcome.root.display(),
            outcome.spec_dir
        );
    } else {
        println!(
            "Initialized spectra project in {} (spec_dir: {}).",
            outcome.root.display(),
            outcome.spec_dir
        );
        if outcome.gitignore_updated {
            println!("Added '.spectra/' to .gitignore.");
        }
    }
    Ok(0)
}

fn cmd_drift(
    cfg: &Config,
    change_name: Option<&str>,
    as_json: bool,
    use_color: bool,
) -> Result<i32> {
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

fn cmd_analyze(cfg: &Config, change_name: Option<&str>, as_json: bool) -> Result<i32> {
    let name = change::resolve(cfg, change_name)?;
    let report = analyze::analyze(cfg, &name)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", analyze::format_human(&report));
    }
    Ok(0)
}

fn cmd_validate(
    cfg: &Config,
    change_name: Option<&str>,
    all_changes: bool,
    strict: bool,
    as_json: bool,
) -> Result<i32> {
    let report = if all_changes {
        spectra_core::validate::validate_all_active(cfg, strict)?
    } else {
        // A single named (or auto-detected) change. `resolve` yields the
        // reference CLI's "no active changes" / "multiple changes" errors on
        // the auto-detect (None) path, but passes an explicit name through
        // verbatim -- so verify it actually exists here, otherwise a typo'd,
        // parked, or archived name would be silently reported as a bogus "no
        // delta" validation failure instead of "Change '<name>' not found."
        // (matching `archive`).
        let name = change::resolve(cfg, change_name)?;
        if change::try_load(cfg, &name)?.is_none() {
            anyhow::bail!("Change '{name}' not found.");
        }
        spectra_core::validate::build_report(cfg, std::slice::from_ref(&name), strict)?
    };

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_validate_human(&report);
    }
    // Gate semantics (distinct from `drift`, which always exits 0): a failed
    // validation exits non-zero so `validate` can back a CI check directly.
    Ok(i32::from(report.any_failed()))
}

fn print_validate_human(report: &spectra_core::validate::ValidateReport) {
    for item in &report.items {
        if item.valid {
            println!("{:<45} OK", item.id);
        } else {
            let n = item.issues.len();
            let noun = if n == 1 { "issue" } else { "issues" };
            println!("{:<45} FAIL ({n} {noun})", item.id);
            for issue in &item.issues {
                println!("  {} {}: {}", issue.level, issue.path, issue.message);
            }
        }
    }
    let t = &report.summary.totals;
    println!(
        "\n{} passed, {} failed ({} total).",
        t.passed, t.failed, t.total
    );
}

/// Whether to emit ANSI color codes: the `--no-color` flag and the `NO_COLOR`
/// env var (https://no-color.org — "when present, **regardless of its
/// value**") both disable it; otherwise color is only emitted when stdout is
/// a terminal (never when piped/redirected).
fn color_enabled(no_color: bool) -> bool {
    color_enabled_from(
        no_color,
        std::env::var_os("NO_COLOR").is_some(),
        std::io::stdout().is_terminal(),
    )
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
    let names = if want_parked {
        change::list_parked(cfg)
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
        println!(
            "{}",
            if want_parked {
                "No parked changes."
            } else {
                "No active changes."
            }
        );
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
        println!(
            "{}",
            serde_json::to_string_pretty(&show_json(item, &content))?
        );
    } else {
        let (ShowContent::Proposal(text) | ShowContent::Spec(text)) = &content;
        print!("{text}");
    }
    Ok(0)
}

/// The exact wrapper shape is the documented `--json` contract for
/// `park`/`unpark`; pin it here so a copy/paste between the two (forgetting
/// to flip `parked`) doesn't ship silently.
fn park_status_json(name: &str, parked: bool) -> serde_json::Value {
    json!({ "name": name, "parked": parked })
}

fn cmd_park(cfg: &Config, name: &str, as_json: bool) -> Result<i32> {
    change::park(cfg, name)?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&park_status_json(name, true))?
        );
    } else {
        println!("Parked '{name}'.");
    }
    Ok(0)
}

fn cmd_unpark(cfg: &Config, name: &str, as_json: bool) -> Result<i32> {
    change::unpark(cfg, name)?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&park_status_json(name, false))?
        );
    } else {
        println!("Unparked '{name}'.");
    }
    Ok(0)
}

fn cmd_in_progress_add(cfg: &Config, name: &str) -> Result<i32> {
    change::mark_in_progress(cfg, name)?;
    Ok(0)
}

/// Render one schema's human line, e.g.
/// `  spec-driven (package) — <description>`, dimming the `(source)` tag when
/// color is enabled (matching the oracle's `\x1b[2m` faint styling). Pulled out
/// so the color wiring is unit-testable without spawning the binary.
fn schema_line(schema: &schema::SchemaListing, use_color: bool) -> String {
    format!(
        "  {} {} — {}",
        schema.name,
        colorize(&format!("({})", schema.source), "2", use_color),
        schema.description
    )
}

/// Render the full human `schemas` output (bold header + one dimmed line per
/// schema), newline-terminated per line. Kept as a helper — rather than
/// inlining the `println!`s in `cmd_schemas` — so the actual header/source SGR
/// *wiring* (bold `\x1b[1m` header, dim `\x1b[2m` source) is byte-for-byte
/// testable with color enabled; the golden integration tests only reach the
/// `--no-color` path, so a `"1"`->`"2"` header regression would otherwise slip.
fn render_schemas_human(schemas: &[schema::SchemaListing], use_color: bool) -> String {
    // The oracle bolds the header (\x1b[1m) and dims each (source) tag (\x1b[2m).
    let mut out = format!("{}\n", colorize("Available schemas:", "1", use_color));
    for schema in schemas {
        out.push_str(&schema_line(schema, use_color));
        out.push('\n');
    }
    out
}

fn cmd_schemas(as_json: bool, use_color: bool) -> Result<i32> {
    let schemas = schema::schemas();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&schemas)?);
    } else {
        print!("{}", render_schemas_human(&schemas, use_color));
    }
    Ok(0)
}

fn cmd_status(
    cfg: &Config,
    change_name: Option<&str>,
    schema_name: Option<&str>,
    as_json: bool,
) -> Result<i32> {
    let report = schema::status(cfg, change_name, schema_name)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }

    println!("Change: {}", report.change_name);
    println!("Schema: {}", report.schema_name);
    println!();
    for artifact in &report.artifacts {
        let marker = match artifact.status {
            schema::ArtifactState::Done => "✓",
            schema::ArtifactState::Ready => "○",
            schema::ArtifactState::Blocked => "✗",
        };
        println!("  {marker} {} ({})", artifact.id, artifact.output_path);
        if let Some(missing_deps) = &artifact.missing_deps {
            println!("    blocked by: {}", missing_deps.join(", "));
        }
    }
    println!();
    if report.is_complete {
        println!("  ✓ All artifacts complete");
    }
    Ok(0)
}

fn artifact_instructions_human(report: &instructions::ArtifactInstructions) -> String {
    let mut output = format!(
        "Artifact: {}\nOutput: {}\nDescription: {}\n\nInstruction:\n{}\n\n",
        report.artifact_id, report.output_path, report.description, report.instruction
    );
    if !report.dependencies.is_empty() {
        output.push_str("Dependencies:\n");
        for dependency in &report.dependencies {
            let glyph = if dependency.done { "✓" } else { "○" };
            output.push_str(&format!(
                "  {glyph} {} ({})\n",
                dependency.id, dependency.path
            ));
        }
        output.push('\n');
    }
    if !report.unlocks.is_empty() {
        output.push_str("Unlocks:\n");
        for artifact_id in &report.unlocks {
            output.push_str(&format!("  - {artifact_id}\n"));
        }
        output.push('\n');
    }
    output.push_str("Template:\n");
    output.push_str(report.template);
    output.push('\n');
    output
}

fn apply_instructions_human(report: &instructions::ApplyInstructions) -> String {
    let mut output = format!(
        "Change: {}\nSchema: {}\nState: {}\nProgress: {}/{} complete\n\n",
        report.change_name,
        report.schema_name,
        match report.state {
            instructions::ApplyState::Blocked => "blocked",
            instructions::ApplyState::AllDone => "all_done",
            instructions::ApplyState::Ready => "ready",
        },
        report.progress.complete,
        report.progress.total
    );
    if !report.missing_artifacts.is_empty() {
        output.push_str("Missing artifacts:\n");
        for artifact in &report.missing_artifacts {
            output.push_str(&format!("  - {artifact}\n"));
        }
        output.push('\n');
    } else if !report.tasks.is_empty() {
        output.push_str("Tasks:\n");
        for task in &report.tasks {
            let glyph = if task.done { "✓" } else { "○" };
            output.push_str(&format!("  {glyph} {}\n", task.description));
        }
        output.push('\n');
    }
    // The oracle's zero-checkbox human layout was not probed. Keep both
    // optional sections absent when neither parsed tasks nor artifacts exist.
    output.push_str("Instruction:\n");
    output.push_str(report.instruction);
    output.push('\n');
    output
}

fn cmd_instructions(
    cfg: &Config,
    artifact_id: Option<&str>,
    change_name: Option<&str>,
    schema_name: Option<&str>,
    as_json: bool,
) -> Result<i32> {
    let report = instructions::get(cfg, change_name, schema_name, artifact_id)?;
    let output = if as_json {
        format!("{}\n", serde_json::to_string_pretty(&report)?)
    } else {
        match &report {
            instructions::InstructionOutput::Artifact(report) => {
                artifact_instructions_human(report)
            }
            instructions::InstructionOutput::Apply(report) => apply_instructions_human(report),
        }
    };
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    stdout
        .write_all(output.as_bytes())
        .context("writing instructions output")?;
    Ok(0)
}

/// `--json` shape for `new change`: pinned here (rather than inlined in
/// `cmd_new_change`) so a rename/typo doesn't ship silently, matching
/// `show_json`/`park_status_json`. Renders `dir` via `to_string_lossy`
/// instead of serializing the `PathBuf` directly: `json!`'s `PathBuf`
/// support requires valid UTF-8 and panics (not a recoverable error) on a
/// path that isn't, which `to_string_lossy` avoids for that rare case.
fn new_change_json(ch: &change::Change) -> serde_json::Value {
    json!({
        "name": ch.name,
        "dir": ch.dir.to_string_lossy(),
        "started_sha": ch.started_sha,
    })
}

fn cmd_new_change(cfg: &Config, name: &str, as_json: bool) -> Result<i32> {
    let ch = change::create(cfg, name)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&new_change_json(&ch))?);
    } else {
        println!("Created change '{}' in {}.", ch.name, ch.dir.display());
        if ch.started_sha.is_none() {
            eprintln!(
                "note: couldn't determine a git baseline for this change; \
                 task-blocked detection will be skipped for it."
            );
        }
    }
    Ok(0)
}

fn new_artifact_json(outcome: &artifact::NewArtifactOutcome) -> serde_json::Value {
    json!({
        "artifact": outcome.artifact,
        "change": outcome.change,
        "path": outcome.path.to_string_lossy(),
        "status": "created",
        "validated": outcome.validated,
        "warnings": outcome.warnings,
    })
}

fn cmd_new_artifact(
    cfg: &Config,
    type_name: &str,
    capability: Option<&str>,
    change_name: Option<&str>,
    from_stdin: bool,
    force: bool,
    as_json: bool,
) -> Result<i32> {
    let stdin_content = if from_stdin {
        let mut content = String::new();
        std::io::stdin()
            .read_to_string(&mut content)
            .context("reading stdin")?;
        Some(content)
    } else {
        None
    };
    let outcome = artifact::create(
        cfg,
        type_name,
        capability,
        change_name,
        stdin_content.as_deref(),
        force,
    )?;

    if as_json {
        println!("{}", serde_json::to_string(&new_artifact_json(&outcome))?);
    } else {
        println!("✓ Created {}: {}", outcome.artifact, outcome.path.display());
        if outcome.validated {
            println!("  Content validated ✓");
        }
    }
    Ok(0)
}

/// `--json` shape for `task done`, reverse-engineered against
/// `/Applications/Spectra.app` v2.3.1: `{"change","status","task_desc","task_id"}`,
/// `task_id` rendered as a string (matching the reference CLI exactly).
fn task_done_json(outcome: &change::TaskDoneOutcome) -> serde_json::Value {
    json!({
        "change": outcome.change,
        "status": "done",
        "task_desc": outcome.task_desc,
        "task_id": outcome.task_id.to_string(),
    })
}

/// Parses the raw `<TASK_ID>` CLI argument, producing the reference CLI's
/// exact error wording on a non-numeric input. Pulled out so this check
/// (which runs before any change lookup) is unit-testable without a `Config`.
fn parse_task_id(raw: &str) -> Result<usize> {
    raw.parse()
        .map_err(|_| anyhow::anyhow!("Invalid task ID '{raw}': must be a number"))
}

fn cmd_task_done(
    cfg: &Config,
    change_name: Option<&str>,
    task_id_raw: &str,
    as_json: bool,
) -> Result<i32> {
    let task_id = parse_task_id(task_id_raw)?;
    let name = change::resolve(cfg, change_name)?;
    let outcome = change::mark_task_done(cfg, &name, task_id)?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&task_done_json(&outcome))?
        );
    } else {
        println!(
            "Task {} marked as done: {}",
            outcome.task_id, outcome.task_desc
        );
    }
    Ok(0)
}

fn cmd_archive(
    cfg: &Config,
    change_name: Option<&str>,
    skip_specs: bool,
    mark_tasks_complete: bool,
) -> Result<i32> {
    let name = change::resolve(cfg, change_name)?;
    let outcome = spectra_core::archive::archive(cfg, &name, skip_specs, mark_tasks_complete)?;
    println!(
        "Archived '{}' as '{}'.",
        outcome.name, outcome.archived_name
    );
    for applied in &outcome.specs_applied {
        println!(
            "Specs applied: {} (added: {}, modified: {}, removed: {}, renamed: {})",
            applied.capability, applied.added, applied.modified, applied.removed, applied.renamed
        );
    }
    Ok(0)
}

/// `update` 的人類輸出行，抽出來讓 colorize 接線可單元測試（同
/// `conclusion_line` 的作法）。措辭與著色逐字 pin 自 oracle 2.3.1：
/// 綠色 `✓` / 黃色 `!` 只包符號本身。
fn update_result_line(ids: &[&str], use_color: bool) -> String {
    if ids.is_empty() {
        format!(
            "{} No AI tool configurations found. Use 'spectra init --tools' to set up.",
            colorize("!", "33", use_color)
        )
    } else {
        format!(
            "{} Updated instruction files for: {}",
            colorize("✓", "32", use_color),
            ids.join(", ")
        )
    }
}

fn cmd_update(cfg: &Config, _force: bool, use_color: bool) -> Result<i32> {
    // --force 只為 CLI parity 收下：對 oracle 2.3.1 實測有無 --force 行為
    // 完全相同（update 本來就無條件重寫每個管理檔），見
    // docs/reverse-engineering/update.md 的 --force 一節。
    let ids = spectra_core::update::update_instruction_files(cfg)?;
    println!("{}", update_result_line(&ids, use_color));
    Ok(0)
}

/// `spectra config <sub>`: manage the global user config file. Output shapes
/// are pinned against the 2.3.1 oracle — see
/// `docs/reverse-engineering/config.md`.
fn cmd_config(target: &ConfigTarget, use_color: bool) -> Result<i32> {
    use spectra_core::global_config as gc;

    let path = gc::config_path()?;
    let check = |text: &str| colorize(text, "32", use_color);
    match target {
        ConfigTarget::Path => {
            println!("{}", path.display());
        }
        ConfigTarget::List { json } => {
            // Display paths read leniently: probed, the oracle prints
            // "No configuration set." for an unreadable file too. Only the
            // write paths surface the I/O error.
            let settings = gc::load_for_display(&path);
            if *json {
                let obj: serde_json::Value = serde_json::Value::Object(
                    settings
                        .iter()
                        .map(|(k, v)| Ok((gc::key_string(k)?, gc::to_json(v)?)))
                        .collect::<Result<serde_json::Map<_, _>>>()?,
                );
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else if settings.is_empty() {
                println!("No configuration set.");
            } else {
                // The oracle sorts the human listing by key (probed), even
                // though its file/JSON key order is arbitrary.
                let mut entries: Vec<(String, String)> = settings
                    .iter()
                    .map(|(k, v)| Ok((gc::key_string(k)?, gc::render_value(v)?)))
                    .collect::<Result<Vec<_>>>()?;
                entries.sort();
                for (key, rendered) in entries {
                    println!("{key} = {rendered}");
                }
            }
        }
        ConfigTarget::Get { key } => {
            let settings = gc::load_for_display(&path);
            let Some(value) = gc::get_value(&settings, key) else {
                anyhow::bail!("Key '{key}' not found.");
            };
            // Null/sequence/mapping renderings end in the YAML serializer's
            // newline, so this prints e.g. `null\n\n` — byte-matching the
            // oracle.
            println!("{}", gc::render_value(value)?);
        }
        ConfigTarget::Set {
            key,
            value,
            string,
            allow_unknown: _,
        } => {
            gc::set_value(&path, key, value, *string)?;
            // Echo the raw CLI argument, not the parsed value (probed:
            // `set parallel_tasks TRUE` echoes `TRUE` but stores `true`).
            println!("{} {key} = {value}", check("\u{2713}"));
        }
        ConfigTarget::Unset { key } => {
            gc::unset_value(&path, key)?;
            println!("{} Removed key: {key}", check("\u{2713}"));
        }
        // `--all` is *not* inert (probed): plain `reset` truncates the file to
        // `{}` (creating it when absent), while `--all` deletes it outright.
        // `-y` is inert in both modes -- neither ever prompts, even on a TTY.
        ConfigTarget::Reset { all, yes: _ } => {
            if *all {
                gc::reset_delete(&path)?;
            } else {
                gc::reset_to_empty(&path)?;
            }
            println!("{} Config reset.", check("\u{2713}"));
        }
        ConfigTarget::Edit => {
            // Probed precedence: $EDITOR (even when empty -- an empty value
            // reaches spawn and fails, it is not treated as unset) -> $VISUAL
            // -> `vi`. The oracle spawns `vi`, not `vim`: with a PATH holding
            // only a `vim`, it errors "Failed to open editor 'vi'".
            let editor = std::env::var_os("EDITOR")
                .or_else(|| std::env::var_os("VISUAL"))
                .unwrap_or_else(|| "vi".into());
            // The oracle creates the directory and seeds a missing file before
            // spawning, so the editor can actually save.
            gc::ensure_editable(&path)?;
            let status = std::process::Command::new(&editor)
                .arg(&path)
                .status()
                .with_context(|| format!("Failed to open editor '{}'", editor.to_string_lossy()))?;
            if !status.success() {
                anyhow::bail!("Editor exited with error.");
            }
        }
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

fn completion_shell(shell: Option<clap_complete::Shell>) -> Result<clap_complete::Shell> {
    shell
        .or_else(clap_complete::Shell::from_env)
        .ok_or_else(|| anyhow::anyhow!("could not detect shell from $SHELL; pass one explicitly"))
}

fn run() -> Result<i32> {
    let cli = Cli::parse();
    let use_color = color_enabled(cli.no_color);
    let cwd = std::env::current_dir().context("getting current directory")?;
    let root = find_root(&cwd);

    match &cli.command {
        Command::Init { adopt, json } => cmd_init(&root, *adopt, *json),
        Command::Drift { change, json } => {
            let cfg = require_initialized(&root)?;
            cmd_drift(&cfg, change.as_deref(), *json, use_color)
        }
        Command::Analyze { change, json } => {
            let cfg = require_initialized(&root)?;
            cmd_analyze(&cfg, change.as_deref(), *json)
        }
        // Completion scripts describe the CLI itself and do not depend on
        // project state, so this deliberately skips `require_initialized`.
        Command::Completion { target } => match target {
            CompletionTarget::Generate { shell } => {
                let shell = completion_shell(*shell)?;
                clap_complete::generate(
                    shell,
                    &mut Cli::command(),
                    "spectra",
                    &mut std::io::stdout(),
                );
                Ok(0)
            }
            CompletionTarget::Install { shell, verbose } => {
                completion::install(completion_shell(*shell)?, *verbose)
            }
            CompletionTarget::Uninstall { shell, yes } => {
                completion::uninstall(completion_shell(*shell)?, *yes)
            }
        },
        // `schemas` lists the built-in schema registry, which needs no
        // project — the oracle lists it outside an initialized project too, so
        // it deliberately skips `require_initialized` (like `init`).
        Command::Schemas { json } => cmd_schemas(*json, use_color),
        Command::Status {
            change,
            schema,
            json,
        } => {
            let cfg = require_initialized(&root)?;
            cmd_status(&cfg, change.as_deref(), schema.as_deref(), *json)
        }
        Command::Instructions {
            artifact,
            change,
            schema,
            json,
            skill,
        } => {
            // Skill bodies are proprietary oracle resources and are not
            // embedded in openspectra. This check intentionally precedes all
            // project/change/schema work so --skill always wins.
            if let Some(skill) = skill {
                anyhow::bail!("Unknown skill: {skill}");
            }
            let cfg = require_initialized(&root)?;
            cmd_instructions(
                &cfg,
                artifact.as_deref(),
                change.as_deref(),
                schema.as_deref(),
                *json,
            )
        }
        Command::Validate {
            change,
            changes,
            strict,
            json,
        } => {
            let cfg = require_initialized(&root)?;
            cmd_validate(&cfg, change.as_deref(), *changes, *strict, *json)
        }
        Command::List {
            // `changes` is unused here on purpose: clap's `conflicts_with_all`
            // already rejects it alongside --specs/--parked, so whenever it's
            // true the other two are false and cmd_list's default branch
            // (active changes) already produces the same output -- see
            // design.md's US-002 section.
            changes: _,
            specs,
            parked,
            json,
        } => {
            let cfg = require_initialized(&root)?;
            cmd_list(&cfg, *specs, *parked, *json)
        }
        Command::Show { item, json } => {
            let cfg = require_initialized(&root)?;
            cmd_show(&cfg, item, *json)
        }
        Command::Park { change, json } => {
            let cfg = require_initialized(&root)?;
            cmd_park(&cfg, change, *json)
        }
        Command::Unpark { change, json } => {
            let cfg = require_initialized(&root)?;
            cmd_unpark(&cfg, change, *json)
        }
        Command::InProgress { target } => match target {
            InProgressTarget::Add { name } => {
                let cfg = require_initialized(&root)?;
                cmd_in_progress_add(&cfg, name)
            }
        },
        Command::New { target } => match target {
            NewTarget::Change { name, json } => {
                let cfg = require_initialized(&root)?;
                cmd_new_change(&cfg, name, *json)
            }
            NewTarget::Artifact {
                type_name,
                capability,
                change,
                stdin,
                force,
                json,
            } => {
                let cfg = require_initialized(&root)?;
                cmd_new_artifact(
                    &cfg,
                    type_name,
                    capability.as_deref(),
                    change.as_deref(),
                    *stdin,
                    *force,
                    *json,
                )
            }
        },
        Command::Task { target } => match target {
            TaskTarget::Done {
                task_id,
                change,
                json,
            } => {
                let cfg = require_initialized(&root)?;
                cmd_task_done(&cfg, change.as_deref(), task_id, *json)
            }
        },
        Command::Update { path, force } => {
            // oracle probe（docs/reverse-engineering/update.md）：無 [PATH]
            // 時從 cwd 往上找 `.spectra.yaml`（同其他指令的 find_root）；
            // 給了 [PATH] 就必須「正好是」專案根——oracle 對明確路徑不做
            // walk-up，子目錄會直接 Not initialized。
            let update_root = match path {
                Some(p) => p.clone(),
                None => root.clone(),
            };
            let cfg = require_initialized(&update_root)?;
            cmd_update(&cfg, *force, use_color)
        }
        Command::Archive {
            change,
            skip_specs,
            mark_tasks_complete,
        } => {
            let cfg = require_initialized(&root)?;
            cmd_archive(&cfg, change.as_deref(), *skip_specs, *mark_tasks_complete)
        }
        // Global config management needs no project (like `init`/`schemas`).
        Command::Config { target } => cmd_config(target, use_color),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            // The oracle exits 1 on operational errors (probed: "Change 'x'
            // not found." exits 1); successful drift always exits 0 regardless
            // of severity, so 1 is unambiguously "tool error".
            eprintln!("Error: {e:#}");
            ExitCode::from(1)
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

    #[test]
    fn update_result_line_pins_oracle_wording_and_symbol_only_coloring() {
        // 綠 ✓ / 黃 ! 只包符號（pty probe 對 oracle 2.3.1 的逐位元捕捉）。
        assert_eq!(
            update_result_line(&["claude", "cursor"], true),
            "\x1b[32m✓\x1b[0m Updated instruction files for: claude, cursor"
        );
        assert_eq!(
            update_result_line(&["claude"], false),
            "✓ Updated instruction files for: claude"
        );
        assert_eq!(
            update_result_line(&[], true),
            "\x1b[33m!\x1b[0m No AI tool configurations found. Use 'spectra init --tools' to set up."
        );
        assert_eq!(
            update_result_line(&[], false),
            "! No AI tool configurations found. Use 'spectra init --tools' to set up."
        );
    }

    #[test]
    fn schema_line_dims_the_source_tag_only_when_color_is_enabled() {
        let schema = schema::SchemaListing {
            artifacts: &["proposal", "specs", "design", "tasks"],
            description: "Default OpenSpec workflow - proposal → specs → design → tasks",
            name: "spec-driven",
            source: "package",
        };

        assert_eq!(
            schema_line(&schema, false),
            "  spec-driven (package) — Default OpenSpec workflow - proposal → specs → design → tasks"
        );
        assert_eq!(
            schema_line(&schema, true),
            "  spec-driven \x1b[2m(package)\x1b[0m — Default OpenSpec workflow - proposal → specs → design → tasks"
        );
    }

    #[test]
    fn render_schemas_human_pins_the_full_colored_wiring() {
        // Exercises the real `cmd_schemas` rendering path (header + lines), not
        // just the `colorize` primitive, so a mutation of the *wiring* — e.g.
        // the header SGR `"1"`->`"2"`, or dropping the dim on the source tag —
        // fails here. The golden integration tests only reach the --no-color
        // path (piped stdout), leaving this the sole guard on the colored path.
        let schemas = schema::schemas();

        // --no-color output must equal the two oracle-golden text lines.
        assert_eq!(
            render_schemas_human(&schemas, false),
            "Available schemas:\n  spec-driven (package) — Default OpenSpec workflow - proposal → specs → design → tasks\n"
        );
        // Colored output: bold \x1b[1m header, dim \x1b[2m source tag.
        assert_eq!(
            render_schemas_human(&schemas, true),
            "\x1b[1mAvailable schemas:\x1b[0m\n  spec-driven \x1b[2m(package)\x1b[0m — Default OpenSpec workflow - proposal → specs → design → tasks\n"
        );
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
    fn list_changes_flag_conflicts_with_specs() {
        let err = Cli::try_parse_from(["spectra", "list", "--changes", "--specs"]).unwrap_err();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::ArgumentConflict,
            "expected --changes/--specs to be rejected as conflicting, got: {err}"
        );
    }

    #[test]
    fn list_changes_flag_conflicts_with_parked() {
        let err = Cli::try_parse_from(["spectra", "list", "--changes", "--parked"]).unwrap_err();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::ArgumentConflict,
            "expected --changes/--parked to be rejected as conflicting, got: {err}"
        );
    }

    #[test]
    fn list_changes_flag_parses_alone() {
        let cli = Cli::try_parse_from(["spectra", "list", "--changes"]).unwrap();
        match cli.command {
            Command::List {
                changes,
                specs,
                parked,
                json,
            } => {
                assert!(changes);
                assert!(!specs);
                assert!(!parked);
                assert!(!json);
            }
            _ => panic!("expected Command::List"),
        }
    }

    #[test]
    fn init_json_shape_matches_the_documented_contract() {
        let outcome = spectra_core::init::InitOutcome {
            root: PathBuf::from("/tmp/proj"),
            spec_dir: "openspec".to_string(),
            adopted: true,
            gitignore_updated: true,
        };
        let value = init_json(&outcome);
        assert_eq!(value["root"], "/tmp/proj");
        assert_eq!(value["spec_dir"], "openspec");
        assert_eq!(value["adopted"], true);
        assert_eq!(value["gitignore_updated"], true);
    }

    #[test]
    fn list_specs_items_shape_matches_specs_key_contract() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
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
            claude_slash_commands: false,
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
            claude_slash_commands: false,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("shipped")).unwrap();
        std::fs::write(
            cfg.changes_dir().join("shipped").join("proposal.md"),
            "# Shipped\n",
        )
        .unwrap();
        std::fs::create_dir_all(cfg.changes_dir().join("on-hold")).unwrap();
        std::fs::write(
            cfg.changes_dir().join("on-hold").join("proposal.md"),
            "# On hold\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.join(".spectra").join("changes")).unwrap();
        std::fs::write(
            tmp.join(".spectra").join("changes").join("on-hold.parked"),
            "",
        )
        .unwrap();

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
            claude_slash_commands: false,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("shipped")).unwrap();
        std::fs::write(
            cfg.changes_dir().join("shipped").join("proposal.md"),
            "# Shipped\n",
        )
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
            claude_slash_commands: false,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("auth")).unwrap();
        std::fs::write(
            cfg.changes_dir().join("auth").join("proposal.md"),
            "# Auth change\n",
        )
        .unwrap();
        std::fs::create_dir_all(cfg.specs_dir().join("auth")).unwrap();
        std::fs::write(
            cfg.specs_dir().join("auth").join("spec.md"),
            "# Auth spec\n",
        )
        .unwrap();

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
            claude_slash_commands: false,
        };
        std::fs::create_dir_all(cfg.specs_dir().join("billing")).unwrap();
        std::fs::write(
            cfg.specs_dir().join("billing").join("spec.md"),
            "# Billing spec\n",
        )
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
            claude_slash_commands: false,
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
            claude_slash_commands: false,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("broken")).unwrap();
        // A directory named `.openspec.yaml` makes `read_to_string` fail with
        // a real I/O error (cross-platform), unlike the benign
        // "no metadata file present" case `change::load` otherwise handles.
        std::fs::create_dir_all(cfg.changes_dir().join("broken").join(".openspec.yaml")).unwrap();
        // A same-named spec exists too, to prove the real error isn't
        // silently swallowed into a fallback.
        std::fs::create_dir_all(cfg.specs_dir().join("broken")).unwrap();
        std::fs::write(
            cfg.specs_dir().join("broken").join("spec.md"),
            "# Should not be used\n",
        )
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

    #[test]
    fn park_status_json_reflects_parked_true_and_false() {
        assert_eq!(park_status_json("my-change", true)["parked"], true);
        assert_eq!(park_status_json("my-change", false)["parked"], false);
        assert_eq!(park_status_json("my-change", true)["name"], "my-change");
    }

    fn sample_change(dir: PathBuf, started_sha: Option<&str>) -> change::Change {
        change::Change {
            name: "my-change".to_string(),
            dir,
            metadata: Default::default(),
            started_sha: started_sha.map(str::to_string),
            parked: false,
        }
    }

    #[test]
    fn new_change_json_shape_matches_the_documented_contract() {
        let ch = sample_change(PathBuf::from("/tmp/changes/my-change"), Some("abc123"));
        let value = new_change_json(&ch);
        assert_eq!(value["name"], "my-change");
        assert_eq!(value["dir"], "/tmp/changes/my-change");
        assert_eq!(value["started_sha"], "abc123");
    }

    #[test]
    fn new_change_json_started_sha_is_null_when_absent() {
        let ch = sample_change(PathBuf::from("/tmp/changes/my-change"), None);
        assert!(new_change_json(&ch)["started_sha"].is_null());
    }

    #[cfg(unix)]
    #[test]
    fn new_change_json_does_not_panic_on_a_non_utf8_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = PathBuf::from(OsStr::from_bytes(b"/tmp/bad-\xFF-path"));
        let ch = sample_change(dir, None);
        // `json!` panics serializing a non-UTF-8 PathBuf directly; this must
        // not panic, since new_change_json renders `dir` via to_string_lossy.
        let value = new_change_json(&ch);
        assert!(value["dir"].as_str().unwrap().contains("bad-"));
    }

    #[test]
    fn task_done_json_shape_matches_the_documented_contract() {
        let outcome = change::TaskDoneOutcome {
            change: "my-change".to_string(),
            task_id: 3,
            task_desc: "do the thing".to_string(),
        };
        let value = task_done_json(&outcome);
        assert_eq!(value["change"], "my-change");
        assert_eq!(value["status"], "done");
        assert_eq!(value["task_desc"], "do the thing");
        // Matches the reference CLI: task_id is a string, not a number.
        assert_eq!(value["task_id"], "3");
    }

    #[test]
    fn task_done_json_serializes_keys_in_alphabetical_order() {
        // Relies on serde_json's default Value::Map being a BTreeMap (the
        // "preserve_order" feature isn't enabled) -- pinned here so enabling
        // that feature later would fail this test instead of silently
        // changing the --json key order documented as oracle-matching.
        let outcome = change::TaskDoneOutcome {
            change: "my-change".to_string(),
            task_id: 3,
            task_desc: "do the thing".to_string(),
        };
        let serialized = serde_json::to_string(&task_done_json(&outcome)).unwrap();
        assert_eq!(
            serialized,
            r#"{"change":"my-change","status":"done","task_desc":"do the thing","task_id":"3"}"#
        );
    }

    #[test]
    fn parse_task_id_accepts_a_valid_number() {
        assert_eq!(parse_task_id("3").unwrap(), 3);
    }

    #[test]
    fn parse_task_id_rejects_non_numeric_input() {
        let err = parse_task_id("abc").unwrap_err();
        assert_eq!(err.to_string(), "Invalid task ID 'abc': must be a number");
    }

    #[test]
    fn parse_task_id_rejects_negative_numbers() {
        let err = parse_task_id("-1").unwrap_err();
        assert_eq!(err.to_string(), "Invalid task ID '-1': must be a number");
    }
}
