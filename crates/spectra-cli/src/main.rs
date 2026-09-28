//! OpenSpectra CLI: `init`, `drift`, `analyze`, `schemas`, `completion`,
//! `status`, `instructions`, `validate`, `list`, `show`, `park`, `unpark`,
//! `in-progress add`, `new change`, `new artifact`, `task done`, `archive`,
//! `update`, `config`, `search`, `templates`.

mod completion;
#[cfg(test)]
mod template_cli_check;

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use serde_json::json;

use spectra_core::{
    analyze, artifact, change, config::Config, drift, instructions, schema, search, skills, spec,
    templates,
};

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
    /// Scaffold a fresh project: `.spectra.yaml`, `<spec_dir>/config.yaml`,
    /// `<spec_dir>/{changes/archive,specs}/`, and a `.spectra/` entry in
    /// `.gitignore`. Every other command requires this to have run first.
    Init {
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
        #[arg(long)]
        force: bool,
        #[arg(long, value_name = "DIR")]
        dir: Option<String>,
        #[arg(long)]
        adopt: bool,
        #[arg(long)]
        json: bool,
        /// AI tools to generate files for (e.g., claude, cursor).
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,
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
    /// Search Markdown artifacts using dependency-free lexical ranking.
    Search {
        /// Search query string.
        query: String,
        /// Maximum number of results.
        #[arg(long, default_value_t = 10)]
        limit: usize,
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
    /// Show template paths.
    Templates {
        /// Workflow schema name. Only `spec-driven` is built in -- any other
        /// explicit name is an error rather than a silent fallback. Unlike
        /// `status`/`instructions`, an *unset* flag does NOT fall back to a
        /// change's or the project's configured schema: with no flag this
        /// always reports the built-in schema's templates, since no other
        /// schema's templates are ever loaded.
        #[arg(long)]
        schema: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show workflow artifact status for a change.
    Status {
        /// Change name (auto-detects if only one exists).
        #[arg(long, conflicts_with = "all")]
        change: Option<String>,
        /// Show every active change in one stable report.
        #[arg(long)]
        all: bool,
        /// Workflow schema name; overrides both the change's own
        /// `.openspec.yaml` `schema:` and `<spec_dir>/config.yaml`'s. Only
        /// `spec-driven` is built in — any other name is an error rather than
        /// a silent fallback.
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
        /// Workflow schema name; overrides both the change's own
        /// `.openspec.yaml` `schema:` and `<spec_dir>/config.yaml`'s. Only
        /// `spec-driven` is built in — any other name is an error rather than
        /// a silent fallback.
        #[arg(long)]
        schema: Option<String>,
        /// Output as JSON.
        #[arg(long)]
        json: bool,
        /// Embedded skill name (outputs skill body directly).
        #[arg(long)]
        skill: Option<String>,
        /// Target agent used to render an embedded skill body.
        #[arg(long)]
        agent: Option<String>,
        /// Omit task descriptions from apply JSON.
        #[arg(long)]
        compact: bool,
        /// Return only apply state and progress.
        #[arg(long)]
        summary: bool,
        /// Omit artifact project context while retaining contextRef.
        #[arg(long)]
        omit_context: bool,
        /// Proposal template type variant (bug-fix or refactor).
        #[arg(long = "type", value_name = "TYPE")]
        proposal_type: Option<String>,
    },
    /// Validate changes against the OpenSpec structural rules (a change needs
    /// at least one requirement delta; with --strict, each ADDED/MODIFIED
    /// requirement also needs a normative SHALL/MUST and a `#### Scenario:`).
    /// Unlike `drift`, this is a pass/fail gate: it exits non-zero when any
    /// change is invalid.
    Validate {
        /// Change or spec name to validate.
        item: Option<String>,
        /// Validate every active change.
        #[arg(long, conflicts_with_all = ["item", "specs", "all", "archived"])]
        changes: bool,
        /// Validate every canonical spec.
        #[arg(long, conflicts_with_all = ["item", "changes", "all", "archived"])]
        specs: bool,
        /// Validate all active changes and canonical specs.
        #[arg(long, conflicts_with_all = ["item", "changes", "specs", "archived"])]
        all: bool,
        /// Validate that archived changes have no incomplete tasks.
        #[arg(long, conflicts_with_all = ["item", "changes", "specs", "all"])]
        archived: bool,
        /// Select the direct item's type when names are ambiguous.
        #[arg(
            long = "type",
            value_name = "TYPE",
            requires = "item",
            conflicts_with_all = ["changes", "specs", "all", "archived"]
        )]
        item_type: Option<ValidationItemType>,
        /// Select full or findings-only output for an explicit bulk scope.
        #[arg(long, value_name = "REPORT")]
        report: Option<ValidationReportKind>,
        /// Treat content-quality warnings as failures.
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
        /// Sort by: name, modified, created
        #[arg(long, value_name = "SORT", default_value = "modified")]
        sort: ListSort,
    },
    /// Show a change's proposal, or a spec's content if the name isn't a change.
    Show {
        /// Change or spec name to show.
        item: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        diff: bool,
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
    /// Traceability sidecar operations (OpenSpectra-only).
    Trace {
        #[command(subcommand)]
        target: TraceTarget,
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
        #[arg(short = 'y', long = "yes")]
        yes: bool,
        #[arg(long = "no-validate")]
        no_validate: bool,
        /// Preview archive effects without modifying files.
        #[arg(long)]
        preview: bool,
        /// Output preview or execution result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Inspect implementation scope without changing repository state
    Scope {
        /// Check an earlier scope identity without rendering patches; reuse its change/base arguments
        #[arg(long = "check-snapshot", value_name = "ID")]
        check_snapshot: Option<String>,
        /// Limit review to an identified change's implementation
        #[arg(long)]
        change: Option<String>,
        /// Explicit pre-implementation Git revision (takes precedence over stored metadata)
        #[arg(long)]
        base: Option<String>,
        /// Output the read-only scope and captured patches as JSON
        #[arg(long)]
        json: bool,
    },
    /// Config management commands
    Config {
        #[command(subcommand)]
        target: ConfigTarget,
    },
    /// Schema management commands
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ListSort {
    Name,
    Modified,
    Created,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ValidationItemType {
    Change,
    Spec,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ValidationReportKind {
    Full,
    Findings,
}

impl From<ListSort> for change::SortKey {
    fn from(value: ListSort) -> Self {
        match value {
            ListSort::Name => Self::Name,
            ListSort::Modified => Self::Modified,
            ListSort::Created => Self::Created,
        }
    }
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
enum SchemaCommand {
    /// Create a project-local schema.
    Init {
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, value_delimiter = ',')]
        artifacts: Vec<String>,
        #[arg(long, conflicts_with = "no_default")]
        default: bool,
        #[arg(long)]
        no_default: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
    /// Validate one schema, or every project schema when omitted.
    Validate {
        name: Option<String>,
        #[arg(long)]
        verbose: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show where a schema resolves from.
    Which {
        name: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Fork (copy) a schema
    Fork {
        /// Source schema
        source: String,
        /// New schema name
        name: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
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
        /// Description (accepted for oracle compatibility; the oracle does not store it).
        #[arg(long)]
        description: Option<String>,
        /// Workflow schema to use.
        #[arg(long)]
        schema: Option<String>,
        /// AI agent that created this change (e.g., claude, codex, cursor).
        #[arg(long)]
        agent: Option<String>,
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
enum TraceTarget {
    /// Move inline `<!-- @trace -->` footers in canonical specs into each
    /// capability's `spec.trace.yaml` sidecar (idempotent), and report
    /// sidecar requirement names that no longer match the spec.
    Migrate {
        /// Report what would be migrated without writing any file.
        #[arg(long)]
        dry_run: bool,
        /// Write nothing; exit 1 if any spec still has an inline footer, a
        /// stale trace name, an unreadable sidecar, or a pointer to a missing
        /// sidecar (for CI/pre-commit).
        #[arg(long)]
        check: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum TaskTarget {
    /// Capture the Git baseline for a task without modifying tasks.md.
    Start {
        /// Task ID (1-based sequential index across all tasks.md checkboxes).
        task_id: String,
        /// Change name (auto-detects if only one active change exists).
        #[arg(long)]
        change: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Mark a task as done and record touched files.
    Done {
        /// Task ID (1-based sequential index across all tasks.md checkboxes).
        task_id: String,
        /// Change name (auto-detects if only one active change exists).
        #[arg(long)]
        change: Option<String>,
        #[arg(long)]
        json: bool,
        /// Record this path as touched by the task (repeatable).
        #[arg(long = "file", value_name = "PATH")]
        files: Vec<String>,
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

fn cmd_init(
    root: &Path,
    adopt: bool,
    as_json: bool,
    tools: &[String],
    force: bool,
    spec_dir: Option<&str>,
) -> Result<i32> {
    let outcome = spectra_core::init::init_with_tools(root, adopt, tools, force, spec_dir)?;
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
            "✓ Initialized at {}",
            outcome.root.join(&outcome.spec_dir).display()
        );
    }
    if !as_json && !tools.is_empty() {
        println!("Generated files for: {}", tools.join(", "));
    }
    Ok(0)
}

fn resolve_read_change(cfg: &Config, explicit: Option<&str>) -> Result<Option<String>> {
    let name = change::resolve_optional(cfg, explicit)?;
    if name.is_none() {
        println!("{}", change::NO_ACTIVE_CHANGES_MESSAGE);
    }
    Ok(name)
}

fn cmd_drift(
    cfg: &Config,
    change_name: Option<&str>,
    as_json: bool,
    use_color: bool,
) -> Result<i32> {
    let Some(name) = resolve_read_change(cfg, change_name)? else {
        return Ok(0);
    };
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
    let Some(name) = resolve_read_change(cfg, change_name)? else {
        return Ok(0);
    };
    let report = analyze::analyze(cfg, &name)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", analyze::format_human(&report));
    }
    Ok(0)
}

fn cmd_search(cfg: &Config, query: &str, limit: usize, as_json: bool) -> Result<i32> {
    let response = search::search(cfg, query, limit)?;
    if as_json {
        println!("{}", serde_json::to_string(&response)?);
    } else if response.results.is_empty() {
        println!("No results found.");
    } else {
        println!(
            "Found {} results for \"{}\"",
            response.results.len(),
            response.query
        );
        for result in response.results {
            println!("\n{} ({:.4})", result.path, result.score);
            for snippet in result.snippets {
                println!("  {snippet}");
            }
        }
    }
    Ok(0)
}

#[derive(Clone, Copy)]
struct ValidateOptions<'a> {
    item: Option<&'a str>,
    changes: bool,
    specs: bool,
    all: bool,
    archived: bool,
    item_type: Option<ValidationItemType>,
    report_kind: Option<ValidationReportKind>,
    strict: bool,
    as_json: bool,
}

fn cmd_validate(cfg: &Config, options: ValidateOptions<'_>) -> Result<i32> {
    let ValidateOptions {
        item,
        changes,
        specs,
        all,
        archived,
        item_type,
        report_kind,
        strict,
        as_json,
    } = options;
    let bulk = changes || specs || all || archived;
    if report_kind.is_some() && !bulk {
        anyhow::bail!("--report requires --changes, --specs, --all, or --archived");
    }

    let mut report = if archived {
        spectra_core::validate::validate_archived(cfg)?
    } else if bulk {
        let change_names = if changes || all {
            change::list_active(cfg)
        } else {
            Vec::new()
        };
        let spec_names = if specs || all {
            spec::list(cfg)?
        } else {
            Vec::new()
        };
        spectra_core::validate::build_mixed_report(cfg, &change_names, &spec_names, strict)?
    } else if let Some(item) = item {
        let is_change = change::try_load(cfg, item)?.is_some();
        let is_spec = spec::try_load(cfg, item)?.is_some();
        let validation = match item_type {
            Some(ValidationItemType::Change) if is_change => {
                spectra_core::validate::validate_change(cfg, item, strict)?
            }
            Some(ValidationItemType::Spec) if is_spec => {
                spectra_core::validate::validate_spec(cfg, item, strict)?
            }
            Some(ValidationItemType::Change) => anyhow::bail!("Change '{item}' not found."),
            Some(ValidationItemType::Spec) => anyhow::bail!("Spec '{item}' not found."),
            None if is_change && is_spec => {
                anyhow::bail!(
                    "Ambiguous item '{item}' matches both a change and a spec; pass --type change|spec"
                )
            }
            None if is_change => spectra_core::validate::validate_change(cfg, item, strict)?,
            None if is_spec => spectra_core::validate::validate_spec(cfg, item, strict)?,
            None => anyhow::bail!("Change '{item}' not found."),
        };
        spectra_core::validate::report_from_items(cfg, vec![validation])
    } else if change::list_active(cfg).is_empty() {
        spectra_core::validate::report_from_items(cfg, Vec::new())
    } else {
        let name = change::resolve(cfg, None)?;
        spectra_core::validate::build_report(cfg, std::slice::from_ref(&name), strict)?
    };

    let failed = report.any_failed();
    let findings = matches!(report_kind, Some(ValidationReportKind::Findings));
    if findings {
        report.items.retain(|item| !item.issues.is_empty());
    }
    if as_json && findings {
        let scope = if archived {
            "archived"
        } else if all {
            "all"
        } else if changes {
            "changes"
        } else {
            "specs"
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "report": {
                    "kind": "validation-findings",
                    "version": "1.0",
                    "scope": scope,
                    "returnedItems": report.items.len(),
                    "totalItems": report.summary.totals.total,
                },
                "itemFindings": report.items,
                "summary": report.summary,
                "root": report.root,
            }))?
        );
    } else if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_validate_human(&report);
    }
    Ok(i32::from(failed))
}

fn print_validate_human(report: &spectra_core::validate::ValidateReport) {
    for item in &report.items {
        if item.valid {
            println!("{:<45} OK", item.id);
        } else {
            let n = item.issues.len();
            let noun = if n == 1 { "issue" } else { "issues" };
            println!("{:<45} FAIL ({n} {noun})", item.id);
        }
        for issue in &item.issues {
            println!("  {} {}: {}", issue.level, issue.path, issue.message);
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

/// SGR code for the severity word in human `drift` output (bold + color,
/// oracle 3.0.0): green for light, yellow for medium, red for heavy (and any
/// other value, treated as the worst case).
fn severity_sgr_code(severity: &str) -> &'static str {
    match severity {
        "light" => "1;32",
        "medium" => "1;33",
        _ => "1;31",
    }
}

/// Human `drift` report, byte-for-byte oracle 3.0.0 (probes p29/p30 in
/// `docs/reverse-engineering/drift.md`): a four-dimension table, the broken
/// anchors, then severity and the recommended next step. Each table row is
/// `"  " + {dimension:<11} + " " + {status:<35} + " " + {score:>6}`, so an
/// overlong status pushes the score right instead of truncating. On a TTY the
/// oracle bolds the title, the header cells, the Total row and "Severity",
/// colors anchors cyan and reasons dim. `unresolved_anchors` (OpenSpectra's
/// #83 addition) gets its own section only when non-empty.
fn drift_human(r: &drift::DriftReport, use_color: bool) -> String {
    let bold = |text: &str| colorize(text, "1", use_color);
    let mut out = format!("{}: {}\n", bold("Drift Report"), r.change_id);
    if let Some(created) = &r.created {
        out.push_str(&format!("  Created: {created}\n"));
    }
    out.push('\n');
    out.push_str(&format!(
        "  {} {} {}\n",
        bold("Dimension  "),
        bold(&format!("{:<35}", "Status")),
        bold(&format!("{:>6}", "Score"))
    ));
    for d in &r.dimensions {
        let name = format!("{:?}", d.kind);
        let score = if d.contributes_to_total {
            format!("+{}", d.score)
        } else {
            "—".to_string()
        };
        out.push_str(&format!("  {name:<11} {:<35} {score:>6}\n", d.status));
    }
    out.push_str(&format!(
        "  {} {:<35} {}\n",
        bold("Total      "),
        "",
        bold(&format!("{:>6}", r.total_score))
    ));
    let anchor_section = |title: &str, anchors: &mut dyn Iterator<Item = (&str, &str, &str)>| {
        let mut section = format!("\n{}\n", bold(title));
        for (anchor, category, reason) in anchors {
            section.push_str(&format!(
                "  - {} ({category}) — {}\n",
                colorize(anchor, "36", use_color),
                colorize(reason, "2", use_color)
            ));
        }
        section
    };
    if !r.broken_anchors.is_empty() {
        out.push_str(&anchor_section(
            "Broken anchors",
            &mut r
                .broken_anchors
                .iter()
                .map(|a| (a.anchor.as_str(), a.category.as_str(), a.reason.as_str())),
        ));
    }
    if !r.unresolved_anchors.is_empty() {
        out.push_str(&anchor_section(
            "Unresolved anchors",
            &mut r
                .unresolved_anchors
                .iter()
                .map(|a| (a.anchor.as_str(), a.category.as_str(), a.reason.as_str())),
        ));
    }
    out.push_str(&format!(
        "\n{}: {} drift\n> {}\n",
        bold("Severity"),
        colorize(
            &r.severity.to_uppercase(),
            severity_sgr_code(&r.severity),
            use_color
        ),
        colorize(&r.primary_recommendation, "1;36", use_color)
    ));
    out
}

fn print_human(r: &drift::DriftReport, use_color: bool) {
    print!("{}", drift_human(r, use_color));
}

fn list_change_items(
    cfg: &Config,
    want_parked: bool,
    sort_key: change::SortKey,
) -> Result<Vec<serde_json::Value>> {
    let names = if want_parked {
        change::list_parked_sorted(cfg, sort_key)
    } else {
        change::list_active_sorted(cfg, sort_key)
    };
    let mut items = Vec::new();
    for name in &names {
        let ch = change::load(cfg, name)?;
        let (done, total) = task_counts(&ch.tasks_md());
        // A parked entry always reports "parked", even with every task ticked
        // (probed: the oracle reports "done" for the same change while active).
        let status = if want_parked {
            "parked"
        } else if total > 0 && done == total {
            "done"
        } else {
            "in-progress"
        };
        let mut item = json!({
            "name": name,
            "status": status,
            "completedTasks": done,
            "totalTasks": total,
        });
        // 取不到 summary 時省略這個 key（oracle 3.0.0 不輸出 null）。
        if let Some(summary) = change::summary(&ch) {
            item["summary"] = json!(summary);
        }
        items.push(item);
    }
    Ok(items)
}

/// `list` 的人類輸出行（oracle 3.0.0）：`  • name`，tasks.md 存在時加
/// ` [完成/總數]`（空檔也印 `[0/0]`），有 summary 時加 ` — summary`。
fn list_line(item: &serde_json::Value, has_tasks_md: bool) -> String {
    let mut line = format!("  • {}", item["name"].as_str().unwrap_or(""));
    if has_tasks_md {
        line.push_str(&format!(
            " [{}/{}]",
            item["completedTasks"].as_u64().unwrap_or(0),
            item["totalTasks"].as_u64().unwrap_or(0)
        ));
    }
    if let Some(summary) = item["summary"].as_str() {
        line.push_str(&format!(" — {summary}"));
    }
    line
}

fn cmd_list(
    cfg: &Config,
    want_specs: bool,
    want_parked: bool,
    as_json: bool,
    sort_key: change::SortKey,
) -> Result<i32> {
    // clap rejects --specs with --parked (they're `conflicts_with`), so at
    // most one of the two is ever true here.
    if want_specs {
        return cmd_list_specs(cfg, as_json);
    }
    let items = list_change_items(cfg, want_parked, sort_key)?;
    if as_json {
        // The oracle keys the parked listing on "parked", not "changes".
        let key = if want_parked { "parked" } else { "changes" };
        println!("{}", serde_json::to_string_pretty(&json!({ key: items }))?);
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
        println!("{}", if want_parked { "Parked:" } else { "Changes:" });
        for it in &items {
            let name = it["name"].as_str().unwrap_or("");
            let has_tasks_md = change::load(cfg, name)
                .map(|ch| cfg.root.join(ch.tasks_md()).is_file())
                .unwrap_or(false);
            println!("{}", list_line(it, has_tasks_md));
        }
    }
    Ok(0)
}

/// `list --specs` 的條目（oracle 3.0.0）：`id` 是 capability 名稱，`path` 是 spec
/// 目錄正規化後的絕對路徑；只列含 `spec.md` 的目錄，依名稱排序。
fn list_specs_items(cfg: &Config) -> Result<Vec<serde_json::Value>> {
    let names = spec::list(cfg)?;
    let mut items = Vec::new();
    for name in &names {
        let dir = cfg.specs_dir().join(name);
        let path = dir.canonicalize().unwrap_or(dir);
        items.push(json!({ "id": name, "path": path.to_string_lossy() }));
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
        println!("Specs:");
        for it in &items {
            println!("  • {}", it["id"].as_str().unwrap_or(""));
        }
    }
    Ok(0)
}

fn cmd_show(cfg: &Config, item: &str, as_json: bool, with_diff: bool) -> Result<i32> {
    if with_diff {
        if change::try_load(cfg, item)?.is_none() {
            anyhow::bail!("--diff requires a change name");
        }
        let entries = spectra_core::spec_diff::change_diff(cfg, item)?;
        if as_json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "name": item,
                    "diff": entries,
                }))?
            );
        } else {
            for entry in entries {
                println!(
                    "{} {} / {}\n{}",
                    entry.operation, entry.capability, entry.name, entry.diff
                );
                if let Some(warning) = entry.warning {
                    println!("warning: {warning}");
                }
            }
        }
        return Ok(0);
    }

    let view = spectra_core::show::resolve(cfg, item)?;
    if as_json {
        let json = match &view {
            spectra_core::show::View::Change(change) => serde_json::to_string_pretty(change)?,
            spectra_core::show::View::Spec(spec) => serde_json::to_string_pretty(spec)?,
        };
        println!("{json}");
    } else {
        print!("{}", spectra_core::show::render_human(&view));
    }
    Ok(0)
}

fn cmd_park(cfg: &Config, name: &str, as_json: bool) -> Result<i32> {
    change::park(cfg, name)?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "parked": name }))?
        );
    } else {
        println!("Parked change: {name}");
    }
    Ok(0)
}

fn cmd_unpark(cfg: &Config, name: &str, as_json: bool) -> Result<i32> {
    change::unpark(cfg, name)?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "unparked": name }))?
        );
    } else {
        println!("Unparked change: {name}");
    }
    Ok(0)
}

fn cmd_in_progress_add(cfg: &Config, name: &str) -> Result<i32> {
    change::mark_in_progress(cfg, name)?;
    Ok(0)
}

/// Render one schema's human line, e.g.
/// `  spec-driven (package) — <description>` or `  mycustom (project)` (when
/// description is absent), dimming the `(source)` tag when color is enabled
/// (matching the oracle's `\x1b[2m` faint styling). Pulled out so the color
/// wiring is unit-testable without spawning the binary.
fn schema_line(schema: &schema::SchemaListing, use_color: bool) -> String {
    let description = schema
        .description
        .as_deref()
        .map(|description| format!(" — {description}"))
        .unwrap_or_default();
    format!(
        "  {} {}{}",
        schema.name,
        colorize(&format!("({})", schema.source), "2", use_color),
        description
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

fn cmd_schemas(root: &Path, as_json: bool, use_color: bool) -> Result<i32> {
    let cfg = match Config::load(root) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("warning: {e:#}");
            None
        }
    };
    let schemas = schema::schemas(cfg.as_ref());
    if as_json {
        println!("{}", serde_json::to_string_pretty(&schemas)?);
    } else {
        print!("{}", render_schemas_human(&schemas, use_color));
    }
    Ok(0)
}

// Only the word "Templates" is bolded; the "(schema)" suffix and every
// listing line are left uncolored -- pinned against the oracle (see
// docs/reverse-engineering/templates.md "Color").
fn templates_header_line(schema_name: &str, use_color: bool) -> String {
    format!("{} ({schema_name})", colorize("Templates", "1", use_color))
}

fn cmd_templates(
    cfg: &Config,
    schema_name: Option<&str>,
    as_json: bool,
    use_color: bool,
) -> Result<i32> {
    let result = templates::list(cfg, schema_name)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&result.templates)?);
    } else {
        println!("{}", templates_header_line(&result.schema_name, use_color));
        for template in &result.templates {
            // "○" itself is unprobed: every built-in template has content,
            // so a `has_content: false` case was never reproducible against
            // the oracle for v2.3.1 (see templates.md's Text section).
            let marker = if template.has_content { "✓" } else { "○" };
            println!(
                "  {marker} {} → {}",
                template.artifact_id, template.template_name
            );
        }
    }
    Ok(0)
}

fn status_human(report: &schema::StatusReport) -> String {
    let mut output = format!(
        "Change: {}\nSchema: {}\n\n",
        report.change_name, report.schema_name
    );
    for artifact in &report.artifacts {
        let marker = match artifact.status {
            schema::ArtifactState::Done => "✓",
            schema::ArtifactState::Ready => "○",
            schema::ArtifactState::Blocked => "✗",
            schema::ArtifactState::Skipped => "~",
        };
        output.push_str(&format!(
            "  {marker} {} ({})\n",
            artifact.id, artifact.output_path
        ));
        if let Some(missing_deps) = &artifact.missing_deps {
            output.push_str(&format!("    blocked by: {}\n", missing_deps.join(", ")));
        }
    }
    output.push('\n');
    if report.is_complete {
        output.push_str("  ✓ All artifacts complete\n");
    }
    output
}

fn cmd_status(
    cfg: &Config,
    change_name: Option<&str>,
    schema_name: Option<&str>,
    as_json: bool,
) -> Result<i32> {
    let Some(change_name) = resolve_read_change(cfg, change_name)? else {
        return Ok(0);
    };
    let report = schema::status(cfg, Some(&change_name), schema_name)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", status_human(&report));
    }
    Ok(0)
}

enum BatchStatusEntry {
    Report(schema::StatusReport),
    Error {
        change_name: String,
        message: String,
    },
}

fn cmd_status_all(cfg: &Config, schema_name: Option<&str>, as_json: bool) -> Result<i32> {
    let names = change::list_active(cfg);
    let mut entries = Vec::with_capacity(names.len());
    let mut failed = false;
    for name in names {
        match schema::status(cfg, Some(&name), schema_name) {
            Ok(report) => entries.push(BatchStatusEntry::Report(report)),
            Err(error) => {
                failed = true;
                entries.push(BatchStatusEntry::Error {
                    change_name: name,
                    message: error.to_string(),
                });
            }
        }
    }
    if as_json {
        let entries = entries
            .iter()
            .map(|entry| match entry {
                BatchStatusEntry::Report(report) => serde_json::to_value(report),
                BatchStatusEntry::Error {
                    change_name,
                    message,
                } => Ok(json!({
                    "changeName": change_name,
                    "status": [{
                        "severity": "error",
                        "message": message,
                    }],
                })),
            })
            .collect::<serde_json::Result<Vec<_>>>()?;
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "changes": entries,
                "root": cfg.root,
            }))?
        );
    } else {
        for (index, entry) in entries.iter().enumerate() {
            if index > 0 {
                println!();
            }
            match entry {
                BatchStatusEntry::Report(report) => print!("{}", status_human(report)),
                BatchStatusEntry::Error {
                    change_name,
                    message,
                } => println!("Change: {change_name}\nError: {message}"),
            }
        }
    }
    Ok(i32::from(failed))
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
    output.push_str(&report.template);
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
    output.push_str(&report.instruction);
    output.push('\n');
    output
}

fn cmd_instructions(
    cfg: &Config,
    artifact_id: Option<&str>,
    change_name: Option<&str>,
    schema_name: Option<&str>,
    as_json: bool,
    projection: instructions::Projection,
    proposal_type: Option<instructions::ProposalType>,
) -> Result<i32> {
    let Some(change_name) = resolve_read_change(cfg, change_name)? else {
        return Ok(0);
    };
    let report = instructions::get_with(
        cfg,
        Some(&change_name),
        schema_name,
        artifact_id,
        projection,
        proposal_type,
    )?;
    let output = if as_json {
        format!("{}\n", projection.render(&report)?)
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

fn cmd_new_change(
    cfg: &Config,
    name: &str,
    as_json: bool,
    options: change::CreateOptions<'_>,
) -> Result<i32> {
    let ch = change::create_with(cfg, name, options)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&new_change_json(&ch))?);
    } else {
        // oracle 3.0.0 的輸出；`Schema:` 是寫進 `.openspec.yaml` 的值（未驗證）。
        println!("✓ Created change: {}", ch.name);
        println!("  Path: {}", cfg.root.join(&ch.dir).display());
        println!(
            "  Schema: {}",
            ch.metadata
                .schema
                .as_deref()
                .unwrap_or(spectra_core::schema::SCHEMA_NAME)
        );
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
/// `task done --json`（oracle 3.0.0：單行、key 依字母排序；serde_json 的 Value
/// 物件本來就是排序 map）。
fn task_done_json(outcome: &change::TaskDoneOutcome) -> serde_json::Value {
    json!({
        "change": outcome.change,
        "provenance": outcome.provenance,
        "status": "done",
        "task_desc": outcome.task_desc,
        "task_id": outcome.task_id,
        "touched_files": outcome.touched_files,
        "warnings": outcome.warnings,
    })
}

/// `task start --json`（oracle 3.0.0：單行、key 依字母排序）。
fn task_start_json(outcome: &change::TaskStartOutcome) -> serde_json::Value {
    json!({
        "baseline_created": outcome.baseline_created,
        "change": outcome.change,
        "git_tracking_available": outcome.git_tracking_available,
        "status": "started",
        "task_id": outcome.task_id,
        "warnings": outcome.warnings,
    })
}

/// `spectra trace migrate`：只回報需要處理的 spec。一般模式下任何一份遷移
/// 失敗就以 1 結束（其他 spec 照樣處理）；`--check` 不寫檔，只要有任何一份
/// 需要處理就以 1 結束。
fn cmd_trace_migrate(cfg: &Config, dry_run: bool, check: bool, as_json: bool) -> Result<i32> {
    let write_nothing = dry_run || check;
    let report = spectra_core::trace::migrate(cfg, write_nothing)?;
    let failed = if check {
        report.iter().any(|spec| spec.needs_attention())
    } else {
        report.iter().any(|spec| spec.error.is_some())
    };
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "dry_run": dry_run,
                "check": check,
                "specs": report,
            }))?
        );
        return Ok(i32::from(failed));
    }
    if report.is_empty() {
        println!("No inline trace footers or stale trace names found.");
        return Ok(0);
    }
    for spec in &report {
        if let Some(error) = &spec.error {
            eprintln!("error: {}: {error}", spec.capability);
            continue;
        }
        if !spec.stale_names.is_empty() {
            eprintln!(
                "warning: {}: {} names requirement(s) not in spec.md: {} (renamed outside openspectra? fix the sidecar by hand)",
                spec.capability,
                spectra_core::trace::SIDECAR_FILE,
                spec.stale_names.join(", ")
            );
        }
        if spec.footers > 0 {
            let verb = if check {
                "has"
            } else if dry_run {
                "would move"
            } else {
                "moved"
            };
            let target = if check { "not yet in" } else { "into" };
            println!(
                "{}: {verb} {} inline trace footer(s) {target} {}",
                spec.capability,
                spec.footers,
                spectra_core::trace::SIDECAR_FILE
            );
        }
        if !spec.unparsed_lines.is_empty() {
            eprintln!(
                "warning: {}: left {} unrecognized `<!-- @trace` footer(s) in place ({})",
                spec.capability,
                spec.unparsed_lines.len(),
                spectra_core::trace::describe_lines(&spec.unparsed_lines)
            );
        }
    }
    Ok(i32::from(failed))
}

fn cmd_task_done(
    cfg: &Config,
    change_name: Option<&str>,
    task_arg: &str,
    files: &[String],
    as_json: bool,
) -> Result<i32> {
    let name = change::resolve_for_task(cfg, change_name)?;
    let outcome = change::mark_task_done(cfg, &name, task_arg, files)?;
    if as_json {
        println!("{}", serde_json::to_string(&task_done_json(&outcome))?);
    } else {
        println!(
            "✓ Task {} marked as done: {}",
            outcome.task_id, outcome.task_desc
        );
        for warning in &outcome.warnings {
            eprintln!("! {warning}");
        }
    }
    Ok(0)
}

fn cmd_task_start(
    cfg: &Config,
    change_name: Option<&str>,
    task_arg: &str,
    as_json: bool,
) -> Result<i32> {
    let name = change::resolve_for_task(cfg, change_name)?;
    let outcome = change::start_task(cfg, &name, task_arg)?;
    if as_json {
        println!("{}", serde_json::to_string(&task_start_json(&outcome))?);
    } else if !outcome.git_tracking_available {
        eprintln!(
            "! Task {} started without a Git baseline ({})",
            outcome.task_id,
            change::WARNING_GIT_UNAVAILABLE
        );
    } else if outcome.baseline_created {
        println!("✓ Task {} baseline captured", outcome.task_id);
    } else {
        println!(
            "Task {} already has a baseline; preserving the original",
            outcome.task_id
        );
    }
    Ok(0)
}

fn cmd_archive(
    cfg: &Config,
    change_name: Option<&str>,
    skip_specs: bool,
    no_validate: bool,
    mark_tasks_complete: bool,
    yes: bool,
    as_json: bool,
) -> Result<i32> {
    let name = change::resolve(cfg, change_name)?;
    if std::io::stdin().is_terminal() && !yes {
        eprint!("Archive '{name}'? (y/N) ");
        std::io::stderr().flush()?;
        let mut response = String::new();
        std::io::stdin().read_line(&mut response)?;
        if !matches!(response.chars().next(), Some('y' | 'Y')) {
            println!("Aborted.");
            return Ok(0);
        }
    }
    let outcome =
        spectra_core::archive::archive(cfg, &name, skip_specs, no_validate, mark_tasks_complete)?;
    if as_json {
        println!(
            "{}",
            serde_json::to_string(&archive_result_json(cfg, &outcome))?
        );
        return Ok(0);
    }
    // oracle 3.0.0 的第一行；它之後的 `Snapshot created for unarchive support.` 不印，
    // 因為 OpenSpectra 沒有 snapshot 機制（#111）。
    println!("✓ Archived: {} → {}", outcome.name, outcome.archived_name);
    for applied in &outcome.specs_applied {
        println!(
            "Specs applied: {} (added: {}, modified: {}, removed: {}, renamed: {})",
            applied.capability, applied.added, applied.modified, applied.removed, applied.renamed
        );
    }
    Ok(0)
}

/// `archive --json` 的執行結果（oracle 3.0.0 的欄位與順序）。`snapshot_created` 固定為
/// `false`：oracle 會為 unarchive 建立 snapshot，OpenSpectra 沒有這個機制（#111）。
/// `cleanup_warnings` 在已 probe 的情境一律為空。
#[derive(serde::Serialize)]
struct ArchiveResultJson {
    archived_id: String,
    archived_path: String,
    applied_specs: Vec<String>,
    snapshot_created: bool,
    total_added: usize,
    total_modified: usize,
    total_removed: usize,
    total_renamed: usize,
    cleanup_warnings: Vec<String>,
}

fn archive_result_json(
    cfg: &Config,
    outcome: &spectra_core::archive::ArchiveOutcome,
) -> ArchiveResultJson {
    let sum = |f: fn(&spectra_core::archive::SpecApplyResult) -> usize| {
        outcome.specs_applied.iter().map(f).sum::<usize>()
    };
    let archived_path = cfg
        .root
        .join(cfg.changes_dir())
        .join("archive")
        .join(&outcome.archived_name);
    ArchiveResultJson {
        archived_id: outcome.archived_name.clone(),
        archived_path: archived_path.to_string_lossy().into_owned(),
        applied_specs: outcome
            .specs_applied
            .iter()
            .map(|s| s.capability.clone())
            .collect(),
        snapshot_created: false,
        total_added: sum(|s| s.added),
        total_modified: sum(|s| s.modified),
        total_removed: sum(|s| s.removed),
        total_renamed: sum(|s| s.renamed),
        cleanup_warnings: Vec::new(),
    }
}

/// `archive --preview`：不詢問確認、不修改檔案。明確給名稱時直接交給 core，
/// 找不到 change 的訊息才會是 oracle 的 `Change '<name>' does not exist`。
/// `spectra scope`（oracle 3.0.0，docs/reverse-engineering/scope.md）。唯讀；limitations 走 stderr，
/// 不上色（oracle 在 TTY 上也不上色，`--no-color` 無作用）。
fn cmd_scope(
    cfg: &Config,
    opts: spectra_core::scope::ScopeOptions<'_>,
    check_snapshot: Option<&str>,
    as_json: bool,
) -> Result<i32> {
    if let Some(id) = check_snapshot {
        spectra_core::scope::check(cfg, opts, id)?;
        if as_json {
            println!(
                "{}",
                serde_json::to_string(&json!({ "snapshot_id": id, "status": "current" }))?
            );
        } else {
            println!("Scope snapshot is current: {id}");
        }
        return Ok(0);
    }
    let report = spectra_core::scope::capture(cfg, opts)?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let (out, err) = spectra_core::scope::render_human(&report);
        print!("{out}");
        eprint!("{err}");
    }
    Ok(0)
}

fn cmd_archive_preview(cfg: &Config, change_name: Option<&str>, as_json: bool) -> Result<i32> {
    let name = match change_name {
        Some(name) => name.to_string(),
        None => change::resolve(cfg, None)?,
    };
    let preview = spectra_core::archive::preview(cfg, &name)?;
    if as_json {
        println!("{}", serde_json::to_string(&preview)?);
    } else {
        println!("Archive preview: {}", preview.change_id);
        println!("Incomplete tasks: {}", preview.incomplete_tasks);
        if preview.spec_updates.is_empty() {
            println!("Spec updates: none");
        } else {
            println!("Spec updates: {}", preview.spec_updates.len());
        }
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
        Command::Init {
            path,
            force,
            dir,
            adopt,
            json,
            tools,
        } => {
            let init_root = if let Some(path) = path {
                std::fs::create_dir_all(path)
                    .with_context(|| format!("creating {}", path.display()))?;
                path.canonicalize()
                    .with_context(|| format!("canonicalizing {}", path.display()))?
            } else {
                root.clone()
            };
            cmd_init(&init_root, *adopt, *json, tools, *force, dir.as_deref())
        }
        Command::Drift { change, json } => {
            let cfg = require_initialized(&root)?;
            cmd_drift(&cfg, change.as_deref(), *json, use_color)
        }
        Command::Analyze { change, json } => {
            let cfg = require_initialized(&root)?;
            cmd_analyze(&cfg, change.as_deref(), *json)
        }
        Command::Search { query, limit, json } => {
            let cfg = require_initialized(&root)?;
            cmd_search(&cfg, query, *limit, *json)
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
        // oracle 在未初始化的專案外也能列出 schemas，因此刻意不呼叫
        // `require_initialized`（與 `init` 相同）。
        Command::Schemas { json } => cmd_schemas(&root, *json, use_color),
        // Template metadata is embedded in the schema registry and is
        // available outside an initialized project, matching the oracle.
        Command::Templates { schema, json } => {
            let cfg = Config::load(&root)?;
            cmd_templates(&cfg, schema.as_deref(), *json, use_color)
        }
        Command::Status {
            change,
            all,
            schema,
            json,
        } => {
            let cfg = require_initialized(&root)?;
            if *all {
                cmd_status_all(&cfg, schema.as_deref(), *json)
            } else {
                cmd_status(&cfg, change.as_deref(), schema.as_deref(), *json)
            }
        }
        Command::Instructions {
            artifact,
            change,
            schema,
            json,
            skill,
            agent,
            compact,
            summary,
            omit_context,
            proposal_type,
        } => {
            // 與專案無關的旗標檢查先於一切（oracle 3.0.0 probe 的順序）。
            let projection = instructions::Projection {
                compact: *compact,
                summary: *summary,
                omit_context: *omit_context,
            };
            projection.validate_flags(*json, skill.is_some())?;
            let proposal_type = match proposal_type {
                Some(_) if skill.is_some() => {
                    anyhow::bail!("invalid --type combination: --type cannot be used with --skill")
                }
                Some(raw) => Some(instructions::ProposalType::parse(raw)?),
                None => None,
            };
            if agent.is_some() && skill.is_none() {
                anyhow::bail!("--agent requires --skill");
            }
            // The skill lookup intentionally precedes all project/change/
            // schema work (including require_initialized) so --skill always
            // wins, even outside an initialized project — matching the oracle.
            if let Some(skill) = skill {
                let Some(body) = skills::skill_body(skill) else {
                    anyhow::bail!("Unknown skill: {skill}");
                };
                let rendered = match agent {
                    // 未初始化時以預設 spec_dir 代入（oracle 在非專案目錄同樣輸出 `openspec/`）。
                    Some(agent) => {
                        let spec_dir = if Config::is_initialized(&root) {
                            Config::load(&root)?.spec_dir
                        } else {
                            spectra_core::config::DEFAULT_SPEC_DIR.to_string()
                        };
                        skills::render_for_agent(body, agent, &spec_dir)?
                    }
                    None => body.to_string(),
                };
                std::io::stdout().write_all(rendered.as_bytes())?;
                return Ok(0);
            }
            let cfg = require_initialized(&root)?;
            cmd_instructions(
                &cfg,
                artifact.as_deref(),
                change.as_deref(),
                schema.as_deref(),
                *json,
                projection,
                proposal_type,
            )
        }
        Command::Validate {
            item,
            changes,
            specs,
            all,
            archived,
            item_type,
            report,
            strict,
            json,
        } => {
            let cfg = require_initialized(&root)?;
            cmd_validate(
                &cfg,
                ValidateOptions {
                    item: item.as_deref(),
                    changes: *changes,
                    specs: *specs,
                    all: *all,
                    archived: *archived,
                    item_type: *item_type,
                    report_kind: *report,
                    strict: *strict,
                    as_json: *json,
                },
            )
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
            sort,
        } => {
            let cfg = require_initialized(&root)?;
            cmd_list(&cfg, *specs, *parked, *json, (*sort).into())
        }
        Command::Show { item, json, diff } => {
            let cfg = require_initialized(&root)?;
            cmd_show(&cfg, item, *json, *diff)
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
            NewTarget::Change {
                name,
                json,
                description: _,
                schema,
                agent,
            } => {
                let cfg = require_initialized(&root)?;
                let options = change::CreateOptions {
                    schema: schema.as_deref(),
                    agent: agent.as_deref(),
                };
                cmd_new_change(&cfg, name, *json, options)
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
            TaskTarget::Start {
                task_id,
                change,
                json,
            } => {
                let cfg = require_initialized(&root)?;
                cmd_task_start(&cfg, change.as_deref(), task_id, *json)
            }
            TaskTarget::Done {
                task_id,
                change,
                json,
                files,
            } => {
                let cfg = require_initialized(&root)?;
                cmd_task_done(&cfg, change.as_deref(), task_id, files, *json)
            }
        },
        Command::Trace { target } => match target {
            TraceTarget::Migrate {
                dry_run,
                check,
                json,
            } => {
                let cfg = require_initialized(&root)?;
                cmd_trace_migrate(&cfg, *dry_run, *check, *json)
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
            yes,
            no_validate,
            preview,
            json,
        } => {
            let cfg = require_initialized(&root)?;
            if *preview {
                return cmd_archive_preview(&cfg, change.as_deref(), *json);
            }
            cmd_archive(
                &cfg,
                change.as_deref(),
                *skip_specs,
                *no_validate,
                *mark_tasks_complete,
                *yes,
                *json,
            )
        }
        Command::Scope {
            check_snapshot,
            change,
            base,
            json,
        } => {
            let cfg = require_initialized(&root)?;
            let opts = spectra_core::scope::ScopeOptions {
                change: change.as_deref(),
                base: base.as_deref(),
            };
            cmd_scope(&cfg, opts, check_snapshot.as_deref(), *json)
        }
        // Global config management needs no project (like `init`/`schemas`).
        Command::Config { target } => cmd_config(target, use_color),
        Command::Schema { command } => match command {
            SchemaCommand::Init {
                name,
                description,
                artifacts,
                default,
                no_default: _,
                force,
                json,
            } => {
                let cfg = require_initialized(&root)?;
                let outcome = schema::init_schema(
                    &cfg,
                    name,
                    description.as_deref(),
                    artifacts,
                    *default,
                    *force,
                )?;
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "created": true,
                            "path": outcome.target_dir,
                            "schema": outcome.target,
                        }))?
                    );
                } else {
                    println!("\u{2713} Created schema '{}'", outcome.target);
                }
                Ok(0)
            }
            SchemaCommand::Validate {
                name,
                verbose,
                json,
            } => {
                let cfg = require_initialized(&root)?;
                let names = match name {
                    Some(name) => vec![name.clone()],
                    None => schema::project_schema_names(&cfg)?,
                };
                let checks: Vec<_> = names
                    .iter()
                    .map(|name| schema::validate_schema(&cfg, name))
                    .collect();
                if *json {
                    println!("{}", serde_json::to_string_pretty(&checks)?);
                } else {
                    for check in &checks {
                        println!(
                            "{} {}{}",
                            if check.valid { "\u{2713}" } else { "\u{2717}" },
                            check.name,
                            if *verbose {
                                format!(" ({})", check.path)
                            } else {
                                String::new()
                            }
                        );
                        for issue in &check.issues {
                            println!("  {issue}");
                        }
                    }
                }
                Ok(i32::from(checks.iter().any(|check| !check.valid)))
            }
            // oracle 3.0.0：不需要已初始化的專案；沒給名稱時固定查 `spec-driven`（不看
            // config.yaml）；`--all` 在所有 probe 過的情境都沒有作用；找不到也 exit 0。
            SchemaCommand::Which { name, all: _, json } => {
                let cfg = Config::is_initialized(&root)
                    .then(|| Config::load(&root))
                    .transpose()?;
                let name = name.as_deref().unwrap_or(schema::SCHEMA_NAME);
                let which = schema::which_sources(cfg.as_ref(), name);
                if *json {
                    println!("{}", serde_json::to_string_pretty(&which)?);
                } else {
                    println!("Schema: {}", which.name);
                    if which.sources.is_empty() {
                        println!("Not found.");
                    }
                    for (i, source) in which.sources.iter().enumerate() {
                        let marker = if i == 0 { "  → " } else { "    " };
                        println!("{marker}{} ({})", source.path, source.source);
                    }
                }
                Ok(0)
            }
            SchemaCommand::Fork {
                source,
                name,
                force,
                json: _, // oracle accepts --json but output is unchanged (probed v2.3.1)
            } => {
                let cfg = require_initialized(&root)?;
                let outcome = schema::fork(&cfg, source, name.as_deref(), *force)?;
                println!(
                    "\u{2713} Forked '{}' \u{2192} '{}'",
                    outcome.source, outcome.target
                );
                Ok(0)
            }
        },
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
    fn templates_header_line_bolds_only_the_word_templates() {
        assert_eq!(
            templates_header_line("spec-driven", true),
            "\x1b[1mTemplates\x1b[0m (spec-driven)"
        );
        assert_eq!(
            templates_header_line("spec-driven", false),
            "Templates (spec-driven)"
        );
    }

    #[test]
    fn severity_sgr_code_maps_known_and_unknown_severities() {
        assert_eq!(severity_sgr_code("light"), "1;32");
        assert_eq!(severity_sgr_code("medium"), "1;33");
        assert_eq!(severity_sgr_code("heavy"), "1;31");
        assert_eq!(severity_sgr_code("anything-else"), "1;31");
    }

    fn sample_drift(status: &str, broken: usize) -> drift::DriftReport {
        let dim = |kind, status: &str, score, contributes| drift::Dimension {
            kind,
            status: status.to_string(),
            score,
            contributes_to_total: contributes,
        };
        drift::DriftReport {
            dormancy: spectra_core::dormancy::decide(
                None,
                chrono::NaiveDate::from_ymd_opt(2026, 9, 28).unwrap(),
                0,
                spectra_core::dormancy::History::NoRepo,
            ),
            change_id: "c".to_string(),
            created: Some("2026-09-01".to_string()),
            last_commit: None,
            dimensions: vec![
                dim(drift::DimensionKind::Time, status, 2, true),
                dim(
                    drift::DimensionKind::Structure,
                    "12/13 anchors broken",
                    4,
                    true,
                ),
                dim(
                    drift::DimensionKind::Tasks,
                    "0 blocked, 0 maybe-done",
                    0,
                    true,
                ),
                dim(drift::DimensionKind::Environment, "0 commits", 0, false),
            ],
            broken_anchors: (1..=broken)
                .map(|i| spectra_core::anchors::BrokenAnchor {
                    anchor: format!("src/missing{i}.rs"),
                    category: "FilePath".to_string(),
                    reason: "file does not exist".to_string(),
                })
                .collect(),
            unresolved_anchors: vec![],
            tasks_maybe_resolved: vec![],
            tasks_blocked_external: vec![],
            commits_since_created: 0,
            total_score: 6,
            severity: "heavy".to_string(),
            recommended_action: drift::RecommendedAction::for_severity("heavy", "c"),
            primary_recommendation: "spectra archive c --skip-specs".to_string(),
        }
    }

    /// oracle 3.0.0 的 human drift（p29 act-heavy 的純文字、p29 p28-none-invalid 的超長狀態、
    /// p30 的 TTY 上色版本），逐位元組。
    #[test]
    fn drift_human_matches_oracle_bytes() {
        assert_eq!(
            drift_human(&sample_drift("stale (27d)", 2), false),
            "Drift Report: c\n  Created: 2026-09-01\n\n  Dimension   Status                               Score\n  Time        stale (27d)                             +2\n  Structure   12/13 anchors broken                    +4\n  Tasks       0 blocked, 0 maybe-done                 +0\n  Environment 0 commits                                —\n  Total                                                6\n\nBroken anchors\n  - src/missing1.rs (FilePath) — file does not exist\n  - src/missing2.rs (FilePath) — file does not exist\n\nSeverity: HEAVY drift\n> spectra archive c --skip-specs\n"
        );
        let long = drift_human(
            &sample_drift("invalid created date \"notadate\", git unavailable", 0),
            false,
        );
        assert!(
            long.contains(
                "\n  Time        invalid created date \"notadate\", git unavailable     +2\n"
            ),
            "{long}"
        );
        assert_eq!(
            drift_human(&sample_drift("stale (27d)", 1), true),
            "\x1b[1mDrift Report\x1b[0m: c\n  Created: 2026-09-01\n\n  \x1b[1mDimension  \x1b[0m \x1b[1mStatus                             \x1b[0m \x1b[1m Score\x1b[0m\n  Time        stale (27d)                             +2\n  Structure   12/13 anchors broken                    +4\n  Tasks       0 blocked, 0 maybe-done                 +0\n  Environment 0 commits                                —\n  \x1b[1mTotal      \x1b[0m                                     \x1b[1m     6\x1b[0m\n\n\x1b[1mBroken anchors\x1b[0m\n  - \x1b[36msrc/missing1.rs\x1b[0m (FilePath) — \x1b[2mfile does not exist\x1b[0m\n\n\x1b[1mSeverity\x1b[0m: \x1b[1;31mHEAVY\x1b[0m drift\n> \x1b[1;36mspectra archive c --skip-specs\x1b[0m\n"
        );
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
            artifacts: vec![
                "proposal".to_string(),
                "specs".to_string(),
                "design".to_string(),
                "tasks".to_string(),
            ],
            description: Some(
                "Default OpenSpec workflow - proposal → specs → design → tasks".to_string(),
            ),
            name: "spec-driven".to_string(),
            source: "package".to_string(),
        };

        assert_eq!(
            schema_line(&schema, false),
            "  spec-driven (package) — Default OpenSpec workflow - proposal → specs → design → tasks"
        );
        assert_eq!(
            schema_line(&schema, true),
            "  spec-driven \x1b[2m(package)\x1b[0m — Default OpenSpec workflow - proposal → specs → design → tasks"
        );

        let project_schema = schema::SchemaListing {
            artifacts: vec!["proposal".to_string()],
            description: None,
            name: "mycustom".to_string(),
            source: "project".to_string(),
        };
        assert_eq!(schema_line(&project_schema, false), "  mycustom (project)");
        assert_eq!(
            schema_line(&project_schema, true),
            "  mycustom \x1b[2m(project)\x1b[0m"
        );
    }

    #[test]
    fn render_schemas_human_pins_the_full_colored_wiring() {
        // Exercises the real `cmd_schemas` rendering path (header + lines), not
        // just the `colorize` primitive, so a mutation of the *wiring* — e.g.
        // the header SGR `"1"`->`"2"`, or dropping the dim on the source tag —
        // fails here. The golden integration tests only reach the --no-color
        // path (piped stdout), leaving this the sole guard on the colored path.
        let schemas = schema::schemas(None);

        // --no-color output must equal the oracle 3.0.0 golden text lines.
        assert_eq!(
            render_schemas_human(&schemas, false),
            "Available schemas:\n  spec-driven (package) — Default OpenSpec workflow - proposal → specs → tasks (design optional)\n  no-spec (package) — No-spec workflow - proposal -> tasks (design optional)\n"
        );
        // Colored output: bold \x1b[1m header, dim \x1b[2m source tag.
        assert_eq!(
            render_schemas_human(&schemas, true),
            "\x1b[1mAvailable schemas:\x1b[0m\n  spec-driven \x1b[2m(package)\x1b[0m — Default OpenSpec workflow - proposal → specs → tasks (design optional)\n  no-spec \x1b[2m(package)\x1b[0m — No-spec workflow - proposal -> tasks (design optional)\n"
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
                sort,
            } => {
                assert!(changes);
                assert!(!specs);
                assert!(!parked);
                assert!(!json);
                assert!(matches!(sort, ListSort::Modified));
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
        // oracle 3.0.0：只有 `id` 與正規化後的絕對 `path`（spec 目錄，不是 spec.md）。
        assert_eq!(
            items[0].as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["id", "path"]
        );
        assert_eq!(items[0]["id"].as_str(), Some("auth"));
        assert_eq!(
            items[0]["path"].as_str(),
            Some(auth_dir.canonicalize().unwrap().to_string_lossy().as_ref())
        );

        // The exact wrapper key ("specs") is the documented --json contract;
        // pin it here so a typo doesn't ship silently.
        let wrapped = json!({ "specs": items });
        assert_eq!(wrapped["specs"][0]["id"], "auth");
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
        // The parked store hangs off the git common dir, so this needs a repo.
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.to_path_buf())
            .args(["init", "-q"])
            .output()
            .unwrap()
            .status
            .success());
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
        // A parked change lives in the oracle's store, not under changes/.
        let parked_dir = tmp
            .join(".git")
            .join("spectra-app")
            .join("changes")
            .join("on-hold");
        std::fs::create_dir_all(&parked_dir).unwrap();
        std::fs::write(parked_dir.join("proposal.md"), "# On hold\n").unwrap();

        let active = list_change_items(&cfg, false, change::SortKey::Name).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0]["name"].as_str(), Some("shipped"));

        let parked = list_change_items(&cfg, true, change::SortKey::Name).unwrap();
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

        assert!(list_change_items(&cfg, true, change::SortKey::Name)
            .unwrap()
            .is_empty());
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

        match spectra_core::show::resolve(&cfg, "auth").unwrap() {
            spectra_core::show::View::Change(change) => {
                assert_eq!(change.proposal.as_deref(), Some("# Auth change\n"))
            }
            spectra_core::show::View::Spec(_) => {
                panic!("expected the change to take priority over the spec")
            }
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

        match spectra_core::show::resolve(&cfg, "billing").unwrap() {
            spectra_core::show::View::Spec(spec) => {
                assert_eq!(spec.files[0].content, "# Billing spec\n")
            }
            spectra_core::show::View::Change(_) => panic!("expected a spec, not a change"),
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

        assert!(spectra_core::show::resolve(&cfg, "ghost").is_err());
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

        let err = match spectra_core::show::resolve(&cfg, "broken") {
            Err(err) => err,
            Ok(_) => panic!("a real I/O error must not fall back to the spec"),
        };
        assert!(!err.to_string().contains("not found as a change or spec"));
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
    fn task_done_json_is_one_alphabetical_line_like_the_oracle() {
        // serde_json 的 Value 物件是排序 map（沒開 preserve_order）；若日後開了這個
        // feature，這裡會失敗而不是靜默改變 oracle 對齊的 key 順序。
        let outcome = change::TaskDoneOutcome {
            change: "demo".to_string(),
            task_id: "1".to_string(),
            task_desc: "1.1 first".to_string(),
            provenance: Some("task_baseline".to_string()),
            touched_files: vec!["src/a.rs".to_string()],
            warnings: Vec::new(),
        };
        assert_eq!(
            serde_json::to_string(&task_done_json(&outcome)).unwrap(),
            r#"{"change":"demo","provenance":"task_baseline","status":"done","task_desc":"1.1 first","task_id":"1","touched_files":["src/a.rs"],"warnings":[]}"#
        );
        let untracked = change::TaskDoneOutcome {
            provenance: None,
            touched_files: Vec::new(),
            warnings: vec![change::WARNING_NO_BASELINE.to_string()],
            ..outcome
        };
        assert_eq!(
            task_done_json(&untracked)["provenance"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn task_start_json_is_one_alphabetical_line_like_the_oracle() {
        let outcome = change::TaskStartOutcome {
            change: "demo".to_string(),
            task_id: "1".to_string(),
            baseline_created: true,
            git_tracking_available: true,
            warnings: Vec::new(),
        };
        assert_eq!(
            serde_json::to_string(&task_start_json(&outcome)).unwrap(),
            r#"{"baseline_created":true,"change":"demo","git_tracking_available":true,"status":"started","task_id":"1","warnings":[]}"#
        );
    }
}
