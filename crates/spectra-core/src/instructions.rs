//! Instruction payloads for workflow artifacts and the apply phase.

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

static APPLY_TASK_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*[-*+]\s*\[(.)\]\s*(.+)$").unwrap());
static BACKTICK_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"`([^`]*?/[^`]*?\.(?:rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js))`"#)
        .unwrap()
});
static BARE_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?:specs|src|src-tauri|crates|lib|tests|app|public)/[\w\-/]+\.(?:rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js)$",
    )
    .unwrap()
});
static LOOSE_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"([A-Za-z0-9_\-./]+/[A-Za-z0-9_\-./]+\.(?:rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js))",
    )
    .unwrap()
});
static BULLET_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*[-*+]\s+").unwrap());
static ASCII_ANNOTATION_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*\([^)]*\)\s*$").unwrap());

const PROPOSAL_REF_MARKERS: &[&str] = &[
    "affected code:",
    "主要檔案",
    "影響檔案",
    "變更檔案",
    "受影響檔案",
];

pub const LOCALE: &str = "English";
pub const APPLY_INSTRUCTION: &str = "Read context files, work through pending tasks, mark complete as you go.\nPause if you hit blockers or need clarification.\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDependency {
    pub id: &'static str,
    pub done: bool,
    pub path: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactInstructions {
    pub change_name: String,
    pub artifact_id: &'static str,
    pub schema_name: &'static str,
    pub change_dir: String,
    pub output_path: &'static str,
    pub description: &'static str,
    pub instruction: &'static str,
    pub locale: &'static str,
    pub template: &'static str,
    pub dependencies: Vec<ArtifactDependency>,
    pub unlocks: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyTask {
    pub id: String,
    pub description: String,
    pub done: bool,
    pub parallel: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyState {
    Blocked,
    AllDone,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Staleness {
    pub days_old: i64,
    pub is_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextFiles {
    // The oracle uses a hash map and its key order varies. Struct fields make
    // openspectra deterministic in schema order while omitting unfinished files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub specs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub total: usize,
    pub complete: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingFile {
    pub path: String,
    pub referenced_in: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriftedFile {
    pub path: String,
    pub last_commit: String,
    pub change_created: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PreflightStatus {
    Critical,
    Warnings,
    Clean,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    pub status: PreflightStatus,
    pub missing_files: Vec<MissingFile>,
    pub drifted_files: Vec<DriftedFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staleness: Option<Staleness>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInstructions {
    pub change_name: String,
    pub change_dir: String,
    pub schema_name: &'static str,
    pub context_files: ContextFiles,
    pub progress: Progress,
    pub tasks: Vec<ApplyTask>,
    pub state: ApplyState,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_artifacts: Vec<&'static str>,
    pub locale: &'static str,
    pub instruction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preflight: Option<Preflight>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum InstructionOutput {
    Artifact(ArtifactInstructions),
    Apply(ApplyInstructions),
}

fn parse_apply_tasks(markdown: &str) -> Vec<ApplyTask> {
    markdown
        .lines()
        .filter_map(|line| {
            let captures = APPLY_TASK_RE.captures(line)?;
            let raw_description = captures[2].trim();
            let (parallel, description) = raw_description
                .strip_prefix("[P]")
                .map_or((false, raw_description), |description| {
                    (true, description.trim())
                });
            Some((captures[1].to_string(), description.to_string(), parallel))
        })
        .enumerate()
        .map(|(index, (state, description, parallel))| ApplyTask {
            id: (index + 1).to_string(),
            description,
            done: state == "x" || state == "X",
            parallel,
        })
        .collect()
}

fn push_unique(paths: &mut Vec<String>, seen: &mut HashSet<String>, path: &str) {
    if seen.insert(path.to_string()) {
        paths.push(path.to_string());
    }
}

fn backtick_references(markdown: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for captures in BACKTICK_PATH_RE.captures_iter(markdown) {
        push_unique(&mut paths, &mut seen, &captures[1]);
    }
    paths
}

fn proposal_references(markdown: &str) -> Vec<String> {
    let lowercase = markdown.to_ascii_lowercase();
    let Some((marker_start, marker)) = PROPOSAL_REF_MARKERS
        .iter()
        .filter_map(|marker| lowercase.find(marker).map(|start| (start, *marker)))
        .min_by_key(|(start, _)| *start)
    else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let after_marker = marker_start + marker.len();
    let line_end = markdown[after_marker..]
        .find('\n')
        .map_or(markdown.len(), |offset| after_marker + offset);
    let marker_line_remainder = markdown[after_marker..line_end].replace('`', "");
    for captures in LOOSE_PATH_RE.captures_iter(&marker_line_remainder) {
        push_unique(&mut paths, &mut seen, &captures[1]);
    }

    if line_end == markdown.len() {
        return paths;
    }
    for line in markdown[line_end + 1..].lines() {
        if line.trim().starts_with('#') {
            break;
        }
        if line.contains('`') {
            for path in backtick_references(line) {
                push_unique(&mut paths, &mut seen, &path);
            }
            continue;
        }

        let without_bullet = BULLET_RE.replace(line, "");
        let without_annotation = ASCII_ANNOTATION_RE.replace(&without_bullet, "");
        let candidate = without_annotation.trim();
        if BARE_PATH_RE.is_match(candidate) {
            push_unique(&mut paths, &mut seen, candidate);
        }
    }
    paths
}

fn derive_unlocks(
    artifact_id: &'static str,
    done_ids: &HashSet<&'static str>,
) -> Vec<&'static str> {
    if done_ids.contains(artifact_id) {
        return Vec::new();
    }
    crate::schema::ARTIFACTS
        .iter()
        .filter(|artifact| artifact.deps.contains(&artifact_id) && !done_ids.contains(artifact.id))
        .map(|artifact| artifact.id)
        .collect()
}

fn derive_apply_state(total: usize, remaining: usize) -> ApplyState {
    if total == 0 {
        ApplyState::Blocked
    } else if remaining == 0 {
        ApplyState::AllDone
    } else {
        ApplyState::Ready
    }
}

fn derive_staleness(today: chrono::NaiveDate, created: chrono::NaiveDate) -> Staleness {
    let days_old = today.signed_duration_since(created).num_days();
    Staleness {
        days_old,
        is_stale: days_old > 7,
    }
}

fn absolute_change_dir(cfg: &crate::Config, change: &crate::Change) -> PathBuf {
    if change.dir.is_absolute() {
        change.dir.clone()
    } else {
        cfg.root.join(&change.dir)
    }
}

fn done_ids(change_dir: &Path) -> Result<HashSet<&'static str>> {
    let mut ids = HashSet::new();
    for artifact in crate::schema::ARTIFACTS.iter() {
        if crate::schema::artifact_done(artifact, change_dir)? {
            ids.insert(artifact.id);
        }
    }
    Ok(ids)
}

pub fn next_artifact(change_dir: &Path) -> Result<Option<&'static str>> {
    for artifact in crate::schema::ARTIFACTS.iter() {
        if !crate::schema::artifact_done(artifact, change_dir)? {
            return Ok(Some(artifact.id));
        }
    }
    Ok(None)
}

pub fn artifact_instructions(
    cfg: &crate::Config,
    change: &crate::Change,
    artifact_id: &str,
) -> Result<ArtifactInstructions> {
    let artifact = crate::schema::ARTIFACTS
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or_else(|| anyhow::anyhow!("Artifact '{artifact_id}' not found in schema"))?;
    let change_dir = absolute_change_dir(cfg, change);
    let done_ids = done_ids(&change_dir)?;
    let dependencies = artifact
        .deps
        .iter()
        .map(|dependency_id| {
            let dependency = crate::schema::ARTIFACTS
                .iter()
                .find(|candidate| candidate.id == *dependency_id)
                .expect("schema dependency references a known artifact");
            ArtifactDependency {
                id: dependency.id,
                done: done_ids.contains(dependency.id),
                path: dependency.output_path,
                description: dependency.description,
            }
        })
        .collect();

    Ok(ArtifactInstructions {
        change_name: change.name.clone(),
        artifact_id: artifact.id,
        schema_name: crate::schema::SCHEMA_NAME,
        change_dir: change_dir.to_string_lossy().into_owned(),
        output_path: artifact.output_path,
        description: artifact.description,
        instruction: artifact.instruction,
        locale: LOCALE,
        template: artifact.template,
        dependencies,
        unlocks: derive_unlocks(artifact.id, &done_ids),
    })
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn context_files(change_dir: &Path, done_ids: &HashSet<&'static str>) -> ContextFiles {
    let path = |artifact_id: &'static str| {
        if !done_ids.contains(artifact_id) {
            return None;
        }
        let artifact = crate::schema::ARTIFACTS
            .iter()
            .find(|artifact| artifact.id == artifact_id)
            .expect("context file references a known artifact");
        Some(
            change_dir
                .join(artifact.output_path)
                .to_string_lossy()
                .into_owned(),
        )
    };
    ContextFiles {
        proposal: path("proposal"),
        design: path("design"),
        specs: path("specs"),
        tasks: path("tasks"),
    }
}

fn valid_date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()
}

fn is_drifted(last_commit: &str, change_created: NaiveDate) -> bool {
    valid_date(last_commit).is_some_and(|last_commit| last_commit > change_created)
}

fn preflight(
    cfg: &crate::Config,
    change: &crate::Change,
    proposal_text: Option<&str>,
    design_text: Option<&str>,
    tasks_text: Option<&str>,
) -> Preflight {
    let proposal_refs = proposal_text.map(proposal_references).unwrap_or_default();
    let missing_files = proposal_refs
        .iter()
        .filter(|path| !cfg.root.join(path).exists())
        .map(|path| MissingFile {
            path: path.clone(),
            referenced_in: "proposal",
        })
        .collect::<Vec<_>>();

    let mut all_refs = proposal_refs;
    let mut seen: HashSet<String> = all_refs.iter().cloned().collect();
    for text in [design_text, tasks_text].into_iter().flatten() {
        for path in backtick_references(text) {
            push_unique(&mut all_refs, &mut seen, &path);
        }
    }

    let parsed_created = change
        .metadata
        .created
        .as_deref()
        .and_then(valid_date)
        .map(|date| (date, change.metadata.created.as_deref().unwrap()));
    let staleness =
        parsed_created.map(|(created, _)| derive_staleness(Local::now().date_naive(), created));
    let mut drifted_files = Vec::new();
    if let Some((created, created_raw)) = parsed_created.filter(|_| crate::git::is_repo(&cfg.root))
    {
        for path in all_refs {
            if !cfg.root.join(&path).exists() {
                continue;
            }
            let Some(last_commit) = crate::git::last_commit_date(&cfg.root, &path) else {
                continue;
            };
            if is_drifted(&last_commit, created) {
                drifted_files.push(DriftedFile {
                    path,
                    last_commit,
                    change_created: created_raw.to_string(),
                });
            }
        }
    }

    let status = if !missing_files.is_empty() {
        PreflightStatus::Critical
    } else if !drifted_files.is_empty()
        || staleness
            .as_ref()
            .is_some_and(|staleness| staleness.is_stale)
    {
        PreflightStatus::Warnings
    } else {
        PreflightStatus::Clean
    };
    Preflight {
        status,
        missing_files,
        drifted_files,
        staleness,
    }
}

pub fn apply_instructions(
    cfg: &crate::Config,
    change: &crate::Change,
) -> Result<ApplyInstructions> {
    let change_dir = absolute_change_dir(cfg, change);
    let proposal_text = read_optional(&change_dir.join("proposal.md"))?;
    let design_text = read_optional(&change_dir.join("design.md"))?;
    let tasks_text = read_optional(&change_dir.join("tasks.md"))?;
    let tasks = tasks_text
        .as_deref()
        .map(parse_apply_tasks)
        .unwrap_or_default();
    let total = tasks.len();
    let complete = tasks.iter().filter(|task| task.done).count();
    let remaining = total - complete;
    let state = derive_apply_state(total, remaining);
    let done_ids = done_ids(&change_dir)?;
    let missing_artifacts = crate::schema::APPLY_REQUIRES
        .iter()
        .copied()
        .filter(|artifact_id| !done_ids.contains(artifact_id))
        .collect();
    let preflight = (state == ApplyState::Ready).then(|| {
        preflight(
            cfg,
            change,
            proposal_text.as_deref(),
            design_text.as_deref(),
            tasks_text.as_deref(),
        )
    });

    Ok(ApplyInstructions {
        change_name: change.name.clone(),
        change_dir: change_dir.to_string_lossy().into_owned(),
        schema_name: crate::schema::SCHEMA_NAME,
        context_files: context_files(&change_dir, &done_ids),
        progress: Progress {
            total,
            complete,
            remaining,
        },
        tasks,
        state,
        missing_artifacts,
        locale: LOCALE,
        instruction: APPLY_INSTRUCTION,
        preflight,
    })
}

pub fn get(
    cfg: &crate::Config,
    explicit_change: Option<&str>,
    schema_name: Option<&str>,
    artifact_id: Option<&str>,
) -> Result<InstructionOutput> {
    let schema_name = schema_name.unwrap_or(crate::schema::SCHEMA_NAME);
    if schema_name != crate::schema::SCHEMA_NAME {
        anyhow::bail!(
            "Schema not found: Schema '{schema_name}' not found in project, user, or built-in locations"
        );
    }

    let change_name = crate::change::resolve(cfg, explicit_change)?;
    let change = crate::change::try_load(cfg, &change_name)?
        .ok_or_else(|| anyhow::anyhow!("Change '{change_name}' not found."))?;
    let selected = match artifact_id {
        Some(artifact_id) => Some(artifact_id),
        None => next_artifact(&change.dir)?,
    };
    match selected {
        Some("apply") | None => apply_instructions(cfg, &change).map(InstructionOutput::Apply),
        Some(artifact_id) => {
            artifact_instructions(cfg, &change, artifact_id).map(InstructionOutput::Artifact)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_parser_accepts_any_checkbox_state_and_only_x_is_done() {
        let tasks = parse_apply_tasks(
            "- [ ] pending\n- [x] done\n- [X] also done\n- [z] custom\n- [?] unknown\n",
        );

        assert_eq!(tasks.len(), 5);
        assert_eq!(
            tasks.iter().map(|task| task.done).collect::<Vec<_>>(),
            vec![false, true, true, false, false]
        );
        assert_eq!(tasks[4].id, "5");
    }

    #[test]
    fn apply_parser_strips_only_an_adjacent_uppercase_parallel_marker() {
        let tasks = parse_apply_tasks(
            "- [ ] [P] 1.2 spaced\n- [ ][P] adjacent\n- [ ] 1.1 [P] later\n- [ ] [p] lower\n* [ ] star\n+ [ ] plus\n",
        );

        assert_eq!(tasks.len(), 6);
        assert_eq!(tasks[0].description, "1.2 spaced");
        assert!(tasks[0].parallel);
        assert_eq!(tasks[1].description, "adjacent");
        assert!(tasks[1].parallel);
        assert_eq!(tasks[2].description, "1.1 [P] later");
        assert!(!tasks[2].parallel);
        assert_eq!(tasks[3].description, "[p] lower");
        assert!(!tasks[3].parallel);
        assert_eq!(tasks[4].description, "star");
        assert_eq!(tasks[5].description, "plus");
    }

    #[test]
    fn proposal_reference_extraction_matches_marker_and_line_rules() {
        let markdown = concat!(
            "# Intro\n",
            "Affected code: `a/d.json`, plain2/mod.rs, foo.rs, src/no.py\n",
            "\n",
            "- `plain3/mod.rs`\n",
            "- lib/l1.js\n",
            "* app/a1.css (annotation)\n",
            "+ src/s1.tsx\n",
            "- public/page.html（annotation）\n",
            "- tests/t1.rs trailing words\n",
            "- plain2/mod.rs\n",
            "- src/no.py\n",
            "## Stop\n",
            "- src/after.rs\n",
        );

        assert_eq!(
            proposal_references(markdown),
            vec![
                "a/d.json",
                "plain2/mod.rs",
                "plain3/mod.rs",
                "lib/l1.js",
                "app/a1.css",
                "src/s1.tsx",
            ]
        );
    }

    #[test]
    fn proposal_reference_extraction_recognizes_every_oracle_marker() {
        for marker in PROPOSAL_REF_MARKERS {
            let markdown = format!("before\n{marker} src/marker.rs\n");
            assert_eq!(proposal_references(&markdown), vec!["src/marker.rs"]);
        }
        assert!(proposal_references("# Impact\n- src/no-marker.rs\n").is_empty());
    }

    #[test]
    fn backtick_reference_extraction_uses_the_exact_extension_allowlist() {
        let allowed = [
            "rs", "ts", "tsx", "jsx", "svelte", "md", "json", "yaml", "toml", "css", "html", "js",
        ];
        let rejected = ["py", "txt", "go", "yml", "sh", "sql", "vue", "mjs"];
        let markdown = allowed
            .iter()
            .chain(rejected.iter())
            .map(|extension| format!("`src/file.{extension}`"))
            .collect::<Vec<_>>()
            .join(" ");

        assert_eq!(
            backtick_references(&markdown),
            allowed
                .iter()
                .map(|extension| format!("src/file.{extension}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn unlocks_are_direct_unfinished_dependents_of_an_unfinished_artifact() {
        assert_eq!(
            derive_unlocks("proposal", &HashSet::new()),
            vec!["design", "specs"]
        );
        assert!(derive_unlocks("proposal", &HashSet::from(["proposal"])).is_empty());
        assert!(derive_unlocks("specs", &HashSet::from(["tasks"])).is_empty());
        assert!(derive_unlocks("proposal", &HashSet::from(["design", "specs"])).is_empty());
        assert_eq!(
            derive_unlocks("proposal", &HashSet::from(["specs"])),
            vec!["design"]
        );
    }

    #[test]
    fn apply_state_is_blocked_for_zero_tasks_then_all_done_or_ready() {
        assert_eq!(derive_apply_state(0, 0), ApplyState::Blocked);
        assert_eq!(derive_apply_state(2, 0), ApplyState::AllDone);
        assert_eq!(derive_apply_state(2, 1), ApplyState::Ready);
    }

    #[test]
    fn staleness_uses_an_eight_day_threshold_and_does_not_clamp_future_dates() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 18).unwrap();

        assert_eq!(
            derive_staleness(today, chrono::NaiveDate::from_ymd_opt(2026, 7, 11).unwrap()),
            Staleness {
                days_old: 7,
                is_stale: false
            }
        );
        assert!(
            derive_staleness(today, chrono::NaiveDate::from_ymd_opt(2026, 7, 10).unwrap()).is_stale
        );
        assert_eq!(
            derive_staleness(today, chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap()).days_old,
            -14
        );
    }

    #[test]
    fn drift_requires_a_strictly_later_valid_commit_date() {
        let created = chrono::NaiveDate::from_ymd_opt(2026, 7, 10).unwrap();

        assert!(!is_drifted("2026-07-09", created));
        assert!(!is_drifted("2026-07-10", created));
        assert!(is_drifted("2026-07-11", created));
        assert!(!is_drifted("not-a-date", created));
    }
}
