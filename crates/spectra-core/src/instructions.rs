//! Instruction payloads for workflow artifacts and the apply phase.

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

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
    pub id: String,
    pub done: bool,
    pub path: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactInstructions {
    pub change_name: String,
    pub artifact_id: String,
    pub schema_name: String,
    pub change_dir: String,
    pub output_path: String,
    pub description: String,
    pub instruction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// 3.0.0 新增：`fnv1a64:<16 位 hex>:<byte 長度>`，對 trim 後的 `context` 計算；
    /// 只在 `context` 存在時輸出，`--omit-context` 拿掉 `context` 但保留它。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<String>>,
    pub locale: String,
    pub template: String,
    pub dependencies: Vec<ArtifactDependency>,
    pub unlocks: Vec<String>,
}

/// `instructions proposal --type` 的範本變體（oracle 3.0.0）。只替換 `template`，
/// `instruction` 與一般 proposal 相同（已 probe）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalType {
    BugFix,
    Refactor,
}

impl ProposalType {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "bug-fix" => Ok(Self::BugFix),
            "refactor" => Ok(Self::Refactor),
            _ => anyhow::bail!("invalid --type value '{raw}': expected 'bug-fix' or 'refactor'"),
        }
    }

    fn template(self) -> &'static str {
        match self {
            Self::BugFix => include_str!("../assets/templates/proposal-bug-fix.md"),
            Self::Refactor => include_str!("../assets/templates/proposal-refactor.md"),
        }
    }
}

/// `--compact`／`--summary`／`--omit-context`：對 JSON 輸出的投影（oracle 3.0.0）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Projection {
    pub compact: bool,
    pub summary: bool,
    pub omit_context: bool,
}

impl Projection {
    fn names(self) -> Vec<&'static str> {
        [
            (self.compact, "--compact"),
            (self.summary, "--summary"),
            (self.omit_context, "--omit-context"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }

    pub fn is_empty(self) -> bool {
        self.names().is_empty()
    }

    /// 不需要讀專案就能判斷的組合錯誤，依 oracle 的檢查順序：互斥 → 缺 `--json` → 搭配 `--skill`。
    pub fn validate_flags(self, json: bool, skill: bool) -> Result<()> {
        let names = self.names();
        match names.as_slice() {
            [] => Ok(()),
            [_, _, ..] => anyhow::bail!(
                "invalid projection combination: {} cannot be used together",
                names.join(", ")
            ),
            [name] if !json => {
                anyhow::bail!("invalid projection combination: {name} requires --json")
            }
            [name] if skill => {
                anyhow::bail!("invalid projection combination: {name} cannot be used with --skill")
            }
            [_] => Ok(()),
        }
    }

    /// 依解析出的 artifact 判斷；`explicit_artifact` 是使用者給的位置參數。
    fn validate_target(
        self,
        explicit_artifact: Option<&str>,
        selected_is_apply: bool,
    ) -> Result<()> {
        if self.compact && explicit_artifact != Some("apply") {
            anyhow::bail!(
                "invalid projection combination: --compact requires an explicit apply artifact"
            );
        }
        if self.summary && !selected_is_apply {
            anyhow::bail!("invalid projection combination: --summary is only valid for apply");
        }
        if self.omit_context && selected_is_apply {
            anyhow::bail!(
                "invalid projection combination: --omit-context is only valid for artifact instructions"
            );
        }
        Ok(())
    }

    /// 把輸出投影成 JSON。`--compact`／`--omit-context` 的 oracle 輸出 key 依字母排序
    /// （轉成 map 後重新序列化的結果），serde_json 的 `Value` 物件本來就是排序 map。
    /// `--summary` 則維持 `state`、`progress` 的原順序，所以用有序的 struct 序列化。
    pub fn render(self, output: &InstructionOutput) -> Result<String> {
        if self.summary {
            if let InstructionOutput::Apply(apply) = output {
                #[derive(Serialize)]
                struct Summary<'a> {
                    state: &'a ApplyState,
                    progress: &'a Progress,
                }
                let summary = Summary {
                    state: &apply.state,
                    progress: &apply.progress,
                };
                return Ok(serde_json::to_string_pretty(&summary)?);
            }
        }
        let mut value = serde_json::to_value(output)?;
        if let Some(map) = value.as_object_mut() {
            if self.compact {
                map.remove("tasks");
            }
            if self.omit_context {
                map.remove("context");
            }
        }
        if self.compact || self.omit_context {
            return Ok(serde_json::to_string_pretty(&value)?);
        }
        Ok(serde_json::to_string_pretty(output)?)
    }
}

/// 64-bit FNV-1a，供 `contextRef` 使用。
fn fnv1a64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn context_ref(context: &str) -> String {
    format!(
        "fnv1a64:{:016x}:{}",
        fnv1a64(context.as_bytes()),
        context.len()
    )
}

/// 內建 schema 的 instruction／template 以 `{{SPEC_DIR}}specs/` 表示 spec 目錄；
/// `instructions` 輸出時代入（`new artifact` 寫檔則保留字面值，與 oracle 相同）。
pub(crate) fn render_spec_dir(text: &str, spec_dir: &str) -> String {
    text.replace(
        "{{SPEC_DIR}}",
        &format!("{}/", spec_dir.trim_end_matches('/')),
    )
}

/// apply 模式的一個 task（oracle 3.0.0 的欄位與順序；新欄位是 snake_case）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApplyTask {
    pub id: String,
    pub number: Option<String>,
    pub prerequisites: Vec<String>,
    pub unresolved_prerequisites: Vec<String>,
    pub cycle_member: bool,
    pub mixed_format: bool,
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

/// apply 的 `contextFiles`：每個已完成 artifact 的 id → 絕對輸出路徑，自訂 schema 的 id
/// 也列（oracle 3.0.0，探測 p37）。oracle 用 hash map、key 順序每次不同；這裡固定為
/// schema 宣告順序，序列化成 JSON 物件。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextFiles(pub Vec<(String, String)>);

impl Serialize for ContextFiles {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(k, v)| (k, v)))
    }
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
    /// oracle 3.0.0 的第一個 key（與 `drift --json` 的同一個物件）。
    pub dormancy: crate::dormancy::Dormancy,
    pub change_name: String,
    pub change_dir: String,
    pub schema_name: String,
    pub context_files: ContextFiles,
    pub progress: Progress,
    pub tasks: Vec<ApplyTask>,
    pub state: ApplyState,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_artifacts: Vec<String>,
    pub locale: String,
    /// schema 沒有 apply instruction 時為 `null`（oracle 3.0.0，探測 p33）。
    pub instruction: Option<String>,
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
    let parsed: Vec<(bool, crate::tasks::TaskText)> = markdown
        .lines()
        .filter_map(|line| {
            let captures = crate::tasks::CHECKBOX_RE.captures(line)?;
            let raw_description = captures[2].trim();
            // A checkbox whose description is only trailing whitespace
            // (`- [ ] `) is not a task: the regex's `(.+)` backtracks onto the
            // lone space, but the oracle drops the line from numbering. Must
            // stay in lockstep with `tasks::is_task_line` (which backs
            // `task done <id>`), or an id from this list would target a
            // different line there. Decided on the raw description, before the
            // `[P]` strip, so both parsers judge task-ness identically.
            if raw_description.is_empty() {
                return None;
            }
            Some((
                crate::tasks::is_done_marker(&captures[1]),
                crate::tasks::parse_task_text(raw_description),
            ))
        })
        .collect();
    let numbers: HashSet<String> = parsed
        .iter()
        .filter_map(|(_, t)| t.number.clone())
        .collect();
    let cycle = cycle_members(&parsed);
    // 任一 task 有前置宣告（即使全都解析不到）就進入 graph 模式；否則沿用 legacy `[P]`。
    let graph_mode = parsed.iter().any(|(_, t)| !t.prerequisites.is_empty());
    let ready: Vec<bool> = parsed
        .iter()
        .enumerate()
        .map(|(i, (done, text))| {
            !done
                && !cycle[i]
                && text.prerequisites.iter().all(|prereq| {
                    numbers.contains(prereq.as_str())
                        && parsed
                            .iter()
                            .filter(|(_, t)| t.number.as_deref() == Some(prereq.as_str()))
                            .all(|(done, _)| *done)
                })
        })
        .collect();
    let ready_count = ready.iter().filter(|r| **r).count();
    parsed
        .into_iter()
        .enumerate()
        .map(|(index, (done, text))| {
            let unresolved_prerequisites = text
                .prerequisites
                .iter()
                .filter(|p| !numbers.contains(p.as_str()))
                .cloned()
                .collect();
            let parallel = if graph_mode {
                ready[index] && ready_count >= 2
            } else {
                text.legacy_parallel
            };
            ApplyTask {
                id: (index + 1).to_string(),
                mixed_format: text.legacy_parallel && !text.prerequisites.is_empty(),
                number: text.number,
                prerequisites: text.prerequisites,
                unresolved_prerequisites,
                cycle_member: cycle[index],
                description: text.description,
                done,
                parallel,
            }
        })
        .collect()
}

/// 位於前置依賴環上的 task（大小大於 1 的強連通分量，或自我參照）。邊是 task → 與其
/// 前置編號相同的每個 task；完成與否不影響（oracle：環上已完成的 task 仍是 `true`）。
fn cycle_members(parsed: &[(bool, crate::tasks::TaskText)]) -> Vec<bool> {
    let edges: Vec<Vec<usize>> = parsed
        .iter()
        .map(|(_, text)| {
            text.prerequisites
                .iter()
                .flat_map(|prereq| {
                    parsed
                        .iter()
                        .enumerate()
                        .filter(move |(_, (_, t))| t.number.as_deref() == Some(prereq.as_str()))
                        .map(|(j, _)| j)
                })
                .collect()
        })
        .collect();
    // Tarjan 的強連通分量（遞迴深度等於 task 數，tasks.md 規模下無虞）。
    struct Tarjan<'a> {
        edges: &'a [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        member: Vec<bool>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, v: usize) {
            self.index[v] = Some(self.next);
            self.low[v] = self.next;
            self.next += 1;
            self.stack.push(v);
            self.on_stack[v] = true;
            for &w in &self.edges[v] {
                match self.index[w] {
                    None => {
                        self.visit(w);
                        self.low[v] = self.low[v].min(self.low[w]);
                    }
                    Some(iw) if self.on_stack[w] => self.low[v] = self.low[v].min(iw),
                    Some(_) => {}
                }
            }
            if Some(self.low[v]) == self.index[v] {
                let mut component = Vec::new();
                while let Some(w) = self.stack.pop() {
                    self.on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                let cyclic = component.len() > 1 || self.edges[v].contains(&v);
                for w in component {
                    self.member[w] = cyclic;
                }
            }
        }
    }
    let n = parsed.len();
    let mut t = Tarjan {
        edges: &edges,
        index: vec![None; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        next: 0,
        member: vec![false; n],
    };
    for v in 0..n {
        if t.index[v].is_none() {
            t.visit(v);
        }
    }
    t.member
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
    schema: &crate::schema::ResolvedSchema,
    artifact_id: &str,
    done_ids: &HashSet<String>,
) -> Vec<String> {
    if done_ids.contains(artifact_id) {
        return Vec::new();
    }
    schema
        .artifacts
        .iter()
        .filter(|artifact| {
            artifact.deps.iter().any(|dep| dep == artifact_id) && !done_ids.contains(&artifact.id)
        })
        .map(|artifact| artifact.id.clone())
        .collect()
}

/// oracle 3.0.0（探測 p36–p38）：缺 required artifact → blocked；schema 沒有 `apply.tracks`
/// → ready（不計 task）；否則依 tracks 檔的 task：0 個 → blocked、全部完成 → all_done、
/// 其餘 → ready。
fn derive_apply_state(
    missing_artifacts: bool,
    tracked: bool,
    total: usize,
    remaining: usize,
) -> ApplyState {
    if missing_artifacts {
        ApplyState::Blocked
    } else if !tracked {
        ApplyState::Ready
    } else if total == 0 {
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

fn done_ids(schema: &crate::schema::ResolvedSchema, change_dir: &Path) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for artifact in &schema.artifacts {
        if crate::schema::artifact_done_resolved(artifact, change_dir)? {
            ids.insert(artifact.id.clone());
        }
    }
    Ok(ids)
}

pub fn next_artifact(
    schema: &crate::schema::ResolvedSchema,
    change_dir: &Path,
) -> Result<Option<String>> {
    for artifact in &schema.artifacts {
        if !crate::schema::artifact_done_resolved(artifact, change_dir)? {
            return Ok(Some(artifact.id.clone()));
        }
    }
    Ok(None)
}

pub fn artifact_instructions(
    cfg: &crate::Config,
    change: &crate::Change,
    artifact_id: &str,
    schema: &crate::schema::ResolvedSchema,
) -> Result<ArtifactInstructions> {
    let artifact = schema
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or_else(|| anyhow::anyhow!("Artifact '{artifact_id}' not found in schema"))?;
    let change_dir = absolute_change_dir(cfg, change);
    let done_ids = done_ids(schema, &change_dir)?;
    let (context, rules) =
        crate::schema::read_spec_config(cfg).map_or((None, None), |mut config| {
            (
                // A blank context is omitted, not emitted as "": probed, the
                // oracle drops the key for `context: ""` and `context: "   "`
                // alike (same shape as `configured_schema_name`'s blank filter).
                config
                    .context
                    .map(|context| context.trim().to_string())
                    .filter(|context| !context.is_empty()),
                config.rules.remove(&artifact.id),
            )
        });
    let dependencies = artifact
        .deps
        .iter()
        .map(|dependency_id| {
            let dependency = schema
                .artifacts
                .iter()
                .find(|candidate| &candidate.id == dependency_id)
                .expect("schema dependency references a known artifact");
            ArtifactDependency {
                id: dependency.id.clone(),
                done: done_ids.contains(&dependency.id),
                path: dependency.output_path.clone(),
                description: dependency.description.clone(),
            }
        })
        .collect();
    let unlocks = derive_unlocks(schema, &artifact.id, &done_ids);
    let context_ref = context.as_deref().map(context_ref);

    Ok(ArtifactInstructions {
        change_name: change.name.clone(),
        artifact_id: artifact.id.clone(),
        schema_name: schema.name.clone(),
        change_dir: change_dir.to_string_lossy().into_owned(),
        output_path: artifact.output_path.clone(),
        description: artifact.description.clone(),
        instruction: render_spec_dir(&artifact.instruction, &cfg.spec_dir),
        context,
        context_ref,
        rules,
        locale: LOCALE.to_string(),
        template: render_spec_dir(&artifact.template, &cfg.spec_dir),
        dependencies,
        unlocks,
    })
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn context_files(
    schema: &crate::schema::ResolvedSchema,
    change_dir: &Path,
    done_ids: &HashSet<String>,
) -> ContextFiles {
    ContextFiles(
        schema
            .artifacts
            .iter()
            .filter(|artifact| done_ids.contains(&artifact.id))
            .map(|artifact| {
                (
                    artifact.id.clone(),
                    change_dir
                        .join(&artifact.output_path)
                        .to_string_lossy()
                        .into_owned(),
                )
            })
            .collect(),
    )
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
    schema: &crate::schema::ResolvedSchema,
) -> Result<ApplyInstructions> {
    let change_dir = absolute_change_dir(cfg, change);
    let proposal_text = read_optional(&change_dir.join("proposal.md"))?;
    let design_text = read_optional(&change_dir.join("design.md"))?;
    // preflight 掃描的仍是 tasks.md；apply 的 task 清單來自 schema 的 `apply.tracks`
    // （內建 schema 就是 tasks.md，沒有 tracks 時不計 task）。
    let tasks_text = read_optional(&change_dir.join("tasks.md"))?;
    let tracked_text = match &schema.apply_tracks {
        Some(tracks) if tracks == "tasks.md" => tasks_text.clone(),
        Some(tracks) => read_optional(&change_dir.join(tracks))?,
        None => None,
    };
    let tasks = tracked_text
        .as_deref()
        .map(parse_apply_tasks)
        .unwrap_or_default();
    let total = tasks.len();
    let complete = tasks.iter().filter(|task| task.done).count();
    let remaining = total - complete;
    let done_ids = done_ids(schema, &change_dir)?;
    let missing_artifacts: Vec<String> = schema
        .apply_requires
        .iter()
        .filter(|artifact_id| !done_ids.contains(*artifact_id))
        .cloned()
        .collect();
    let state = derive_apply_state(
        !missing_artifacts.is_empty(),
        schema.apply_tracks.is_some(),
        total,
        remaining,
    );
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
        dormancy: crate::dormancy::evaluate(
            &cfg.root,
            &change.dir,
            crate::show::schema_and_created(&change.dir).1.as_deref(),
        ),
        change_name: change.name.clone(),
        change_dir: change_dir.to_string_lossy().into_owned(),
        schema_name: schema.name.clone(),
        context_files: context_files(schema, &change_dir, &done_ids),
        progress: Progress {
            total,
            complete,
            remaining,
        },
        tasks,
        state,
        missing_artifacts,
        locale: LOCALE.to_string(),
        instruction: schema.apply_instruction.clone(),
        preflight,
    })
}

/// `get` 加上 3.0.0 的 projection 與 `--type`。與專案無關的旗標檢查
/// （[`Projection::validate_flags`]、[`ProposalType::parse`]）由呼叫端先做；這裡只做
/// 需要知道解析出哪個 artifact 的檢查，順序在 change 解析之後（oracle 對不存在的
/// change 先報 `Change 'X' not found.`）。
pub fn get_with(
    cfg: &crate::Config,
    explicit_change: Option<&str>,
    schema_name: Option<&str>,
    artifact_id: Option<&str>,
    projection: Projection,
    proposal_type: Option<ProposalType>,
) -> Result<InstructionOutput> {
    // Change first, then the schema gate: probed, the oracle reports
    // `Change 'X' not found.` ahead of the schema error, and the change's own
    // `.openspec.yaml` outranks the project `config.yaml` as the selector.
    let change_name = crate::change::resolve(cfg, explicit_change)?;
    let change = crate::change::try_load(cfg, &change_name)?
        .ok_or_else(|| anyhow::anyhow!("Change '{change_name}' not found."))?;
    let schema = crate::schema::resolve_schema(cfg, schema_name, Some(&change))?;
    let selected = match artifact_id {
        Some(artifact_id) => Some(artifact_id.to_string()),
        None => next_artifact(&schema, &change.dir)?,
    };
    let selected_is_apply = matches!(selected.as_deref(), Some("apply") | None);
    projection.validate_target(artifact_id, selected_is_apply)?;
    if proposal_type.is_some() && selected.as_deref() != Some("proposal") {
        anyhow::bail!("invalid --type combination: --type is only valid for proposal instructions");
    }
    match selected.as_deref() {
        Some("apply") | None => {
            apply_instructions(cfg, &change, &schema).map(InstructionOutput::Apply)
        }
        Some(artifact_id) => {
            let mut report = artifact_instructions(cfg, &change, artifact_id, &schema)?;
            if let Some(variant) = proposal_type {
                report.template = variant.template().to_string();
            }
            Ok(InstructionOutput::Artifact(report))
        }
    }
}

pub fn get(
    cfg: &crate::Config,
    explicit_change: Option<&str>,
    schema_name: Option<&str>,
    artifact_id: Option<&str>,
) -> Result<InstructionOutput> {
    get_with(
        cfg,
        explicit_change,
        schema_name,
        artifact_id,
        Projection::default(),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn project(label: &str) -> (TempDir, crate::Config, crate::Change) {
        let tmp = TempDir::new(label);
        let cfg = crate::Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        let change = crate::Change {
            name: "c1".to_string(),
            dir: std::path::PathBuf::from("openspec/changes/c1"),
            metadata: crate::change::ChangeMetadata::default(),
            started_sha: None,
            parked: false,
        };
        std::fs::create_dir_all(cfg.root.join(&change.dir)).unwrap();
        (tmp, cfg, change)
    }

    fn write_spec_config(cfg: &crate::Config, body: &str) {
        std::fs::write(cfg.root.join(&cfg.spec_dir).join("config.yaml"), body).unwrap();
    }

    #[test]
    fn context_ref_hashes_the_trimmed_context_like_the_oracle() {
        // 兩組值取自 oracle 3.0.0 輸出（單行與含中文的多行 context）。
        assert_eq!(
            context_ref("Project context line."),
            "fnv1a64:3951a95bb8e245c9:21"
        );
        assert_eq!(
            context_ref("Line one.\nLine two 中文."),
            "fnv1a64:9a1756e47a692f85:26"
        );

        let (_tmp, cfg, change) = project("instructions-context-ref");
        write_spec_config(&cfg, "context: |\n  Project context line.\n");
        let with = artifact_instructions(
            &cfg,
            &change,
            "proposal",
            &crate::schema::ResolvedSchema::builtin(),
        )
        .unwrap();
        assert_eq!(
            with.context_ref.as_deref(),
            Some("fnv1a64:3951a95bb8e245c9:21")
        );

        write_spec_config(&cfg, "context: \"   \"\n");
        let blank = artifact_instructions(
            &cfg,
            &change,
            "proposal",
            &crate::schema::ResolvedSchema::builtin(),
        )
        .unwrap();
        assert_eq!(blank.context_ref, None);
    }

    #[test]
    fn artifact_text_renders_the_spec_dir_placeholder() {
        let (_tmp, mut cfg, change) = project("instructions-spec-dir");
        cfg.spec_dir = "docs/spectra".to_string();
        std::fs::create_dir_all(cfg.root.join("docs/spectra")).unwrap();
        let schema = crate::schema::ResolvedSchema::builtin();
        let proposal = artifact_instructions(&cfg, &change, "proposal", &schema).unwrap();
        assert!(proposal
            .instruction
            .contains("Check `docs/spectra/specs/` for existing spec names."));
        assert!(proposal
            .template
            .contains("Use existing spec names from docs/spectra/specs/."));
        assert!(!proposal.template.contains("{{SPEC_DIR}}"));
        let specs = artifact_instructions(&cfg, &change, "specs", &schema).unwrap();
        assert!(specs.instruction.contains(
            "Locate the existing requirement in docs/spectra/specs/<capability>/spec.md"
        ));
    }

    #[test]
    fn proposal_type_parses_only_the_two_oracle_variants() {
        assert_eq!(
            ProposalType::parse("bug-fix").unwrap(),
            ProposalType::BugFix
        );
        assert_eq!(
            ProposalType::parse("refactor").unwrap(),
            ProposalType::Refactor
        );
        assert_eq!(
            ProposalType::parse("BUG-FIX").unwrap_err().to_string(),
            "invalid --type value 'BUG-FIX': expected 'bug-fix' or 'refactor'"
        );
        assert!(ProposalType::BugFix
            .template()
            .starts_with("## Problem\n\n## Root Cause\n"));
        assert!(ProposalType::Refactor
            .template()
            .starts_with("## Summary\n\n## Motivation\n"));
    }

    #[test]
    fn projection_flag_errors_follow_the_oracle_order_and_wording() {
        let p = |compact, summary, omit_context| Projection {
            compact,
            summary,
            omit_context,
        };
        let msg = |r: Result<()>| r.unwrap_err().to_string();
        assert_eq!(
            msg(p(true, true, true).validate_flags(true, false)),
            "invalid projection combination: --compact, --summary, --omit-context cannot be used together"
        );
        assert_eq!(
            msg(p(false, true, true).validate_flags(false, true)),
            "invalid projection combination: --summary, --omit-context cannot be used together"
        );
        assert_eq!(
            msg(p(true, false, false).validate_flags(false, true)),
            "invalid projection combination: --compact requires --json"
        );
        assert_eq!(
            msg(p(false, false, true).validate_flags(true, true)),
            "invalid projection combination: --omit-context cannot be used with --skill"
        );
        assert!(p(false, true, false).validate_flags(true, false).is_ok());
        assert!(Projection::default().validate_flags(false, true).is_ok());

        assert_eq!(
            msg(p(true, false, false).validate_target(None, true)),
            "invalid projection combination: --compact requires an explicit apply artifact"
        );
        assert!(p(true, false, false)
            .validate_target(Some("apply"), true)
            .is_ok());
        assert_eq!(
            msg(p(false, true, false).validate_target(Some("proposal"), false)),
            "invalid projection combination: --summary is only valid for apply"
        );
        assert!(p(false, true, false).validate_target(None, true).is_ok());
        assert_eq!(
            msg(p(false, false, true).validate_target(Some("apply"), true)),
            "invalid projection combination: --omit-context is only valid for artifact instructions"
        );
    }

    #[test]
    fn projections_render_the_oracle_shapes() {
        let (_tmp, cfg, change) = project("instructions-projection");
        write_spec_config(&cfg, "context: |\n  Ctx.\n");
        let dir = cfg.root.join(&change.dir);
        std::fs::write(dir.join("tasks.md"), "- [ ] 1.1 First\n- [x] 1.2 Second\n").unwrap();
        let schema = crate::schema::ResolvedSchema::builtin();

        let apply = InstructionOutput::Apply(apply_instructions(&cfg, &change, &schema).unwrap());
        let summary = Projection {
            summary: true,
            ..Projection::default()
        }
        .render(&apply)
        .unwrap();
        assert_eq!(
            summary,
            "{\n  \"state\": \"ready\",\n  \"progress\": {\n    \"total\": 2,\n    \"complete\": 1,\n    \"remaining\": 1\n  }\n}"
        );

        let compact: serde_json::Value = serde_json::from_str(
            &Projection {
                compact: true,
                ..Projection::default()
            }
            .render(&apply)
            .unwrap(),
        )
        .unwrap();
        assert!(compact.get("tasks").is_none());
        assert!(compact.get("progress").is_some());
        let keys: Vec<&String> = compact.as_object().unwrap().keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(
            keys, sorted,
            "compact keys are alphabetical like the oracle"
        );

        let artifact = InstructionOutput::Artifact(
            artifact_instructions(&cfg, &change, "proposal", &schema).unwrap(),
        );
        let omitted: serde_json::Value = serde_json::from_str(
            &Projection {
                omit_context: true,
                ..Projection::default()
            }
            .render(&artifact)
            .unwrap(),
        )
        .unwrap();
        assert!(omitted.get("context").is_none());
        assert!(omitted.get("contextRef").is_some());
    }

    #[test]
    fn artifact_context_is_trimmed_and_preserves_internal_newlines() {
        let (_tmp, cfg, change) = project("instructions-context");
        write_spec_config(
            &cfg,
            "schema: spec-driven\ncontext: |\n  First line\n  Second line\n",
        );

        let instructions = artifact_instructions(
            &cfg,
            &change,
            "proposal",
            &crate::schema::ResolvedSchema::builtin(),
        )
        .unwrap();

        assert_eq!(
            instructions.context.as_deref(),
            Some("First line\nSecond line")
        );
    }

    #[test]
    fn artifact_blank_context_is_omitted_not_emitted_empty() {
        // Probed: the oracle drops the `context` key entirely for both
        // `context: ""` and a whitespace-only value.
        let (_tmp, cfg, change) = project("instructions-blank-context");
        write_spec_config(&cfg, "schema: spec-driven\ncontext: \"   \"\n");

        let instructions = artifact_instructions(
            &cfg,
            &change,
            "proposal",
            &crate::schema::ResolvedSchema::builtin(),
        )
        .unwrap();
        let value = serde_json::to_value(instructions).unwrap();

        assert!(value.get("context").is_none());
    }

    #[test]
    fn artifact_scalar_context_is_omitted_like_the_oracle() {
        // Probed (2026-08-06, three jails): the oracle omits the `context` key
        // for `context: 123`, `context: true`, and `context: 1.5` alike. The
        // `from_value::<Option<String>>` path fails typed deserialization on
        // non-string scalars (no from_str-style coercion), so the lenient
        // reader already maps them to unset — this pins that, so a future
        // deserializer swap that reintroduces coercion fails loudly.
        for (label, config) in [
            ("number", "schema: spec-driven\ncontext: 123\n"),
            ("bool", "schema: spec-driven\ncontext: true\n"),
            ("float", "schema: spec-driven\ncontext: 1.5\n"),
        ] {
            let (_tmp, cfg, change) = project(&format!("instructions-scalar-context-{label}"));
            write_spec_config(&cfg, config);

            let instructions = artifact_instructions(
                &cfg,
                &change,
                "proposal",
                &crate::schema::ResolvedSchema::builtin(),
            )
            .unwrap();
            let value = serde_json::to_value(instructions).unwrap();

            assert!(value.get("context").is_none(), "case: {label}");
        }
    }

    #[test]
    fn artifact_rules_are_flattened_for_the_matching_artifact() {
        let (_tmp, cfg, change) = project("instructions-rules");
        write_spec_config(
            &cfg,
            "schema: spec-driven\nrules:\n  proposal:\n    - Keep it concise\n    - Name the impact\n  tasks:\n    - Keep tasks small\n",
        );

        let instructions = artifact_instructions(
            &cfg,
            &change,
            "proposal",
            &crate::schema::ResolvedSchema::builtin(),
        )
        .unwrap();

        assert_eq!(
            instructions.rules,
            Some(vec![
                "Keep it concise".to_string(),
                "Name the impact".to_string()
            ])
        );
    }

    #[test]
    fn artifact_rules_key_is_absent_for_a_nonmatching_artifact() {
        let (_tmp, cfg, change) = project("instructions-nonmatching-rules");
        write_spec_config(
            &cfg,
            "schema: spec-driven\nrules:\n  proposal:\n    - Keep it concise\n",
        );

        let instructions = artifact_instructions(
            &cfg,
            &change,
            "tasks",
            &crate::schema::ResolvedSchema::builtin(),
        )
        .unwrap();
        let value = serde_json::to_value(instructions).unwrap();

        assert!(value.get("rules").is_none());
    }

    #[test]
    fn artifact_context_and_rules_keys_are_absent_without_config() {
        let (_tmp, cfg, change) = project("instructions-no-config");

        let instructions = artifact_instructions(
            &cfg,
            &change,
            "proposal",
            &crate::schema::ResolvedSchema::builtin(),
        )
        .unwrap();
        let value = serde_json::to_value(instructions).unwrap();

        assert!(value.get("context").is_none());
        assert!(value.get("rules").is_none());
    }

    #[test]
    fn artifact_context_and_rules_keys_are_absent_for_unparseable_config() {
        let (_tmp, cfg, change) = project("instructions-bad-config");
        write_spec_config(&cfg, "schema: [not, valid\n");

        let instructions = artifact_instructions(
            &cfg,
            &change,
            "proposal",
            &crate::schema::ResolvedSchema::builtin(),
        )
        .unwrap();
        let value = serde_json::to_value(instructions).unwrap();

        assert!(value.get("context").is_none());
        assert!(value.get("rules").is_none());
    }

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
    fn apply_parser_drops_checkboxes_with_only_a_trailing_space() {
        // `- [ ] ` (empty description after trim) is not a task: the oracle
        // drops it from numbering, so it must not be counted or shift ids here.
        // Must stay in lockstep with `tasks::is_task_line` (`task done <id>`).
        let tasks = parse_apply_tasks("- [x] first task\n- [ ] \n- [ ] second real task\n");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].description, "first task");
        assert_eq!(tasks[1].id, "2");
        assert_eq!(tasks[1].description, "second real task");
    }

    #[test]
    fn apply_parser_and_task_done_parser_number_every_marker_style_identically() {
        // An id read from `instructions apply` is fed to `task done <id>`
        // (`tasks::mark_done`); if the two parsers disagree on which lines are
        // tasks, `task done` flips the wrong line (#172).
        let md = "- [x] a\n* [ ] b\n+ [ ] c\n- [~] d\n1. [ ] skip\n- [ x] skip\n  + [X] e\n- [ ] \n- [ ] f\n";
        let apply: Vec<(String, bool)> = parse_apply_tasks(md)
            .into_iter()
            .map(|t| (t.description, t.done))
            .collect();
        let counted: Vec<(String, bool)> = crate::tasks::parse(md)
            .into_iter()
            .map(|t| (t.description, t.done))
            .collect();
        assert_eq!(apply, counted);
        assert_eq!(apply.len(), 6);
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
        let schema = crate::schema::ResolvedSchema::builtin();
        let done: fn(&[&str]) -> HashSet<String> =
            |ids| ids.iter().map(|id| id.to_string()).collect();
        assert_eq!(
            derive_unlocks(&schema, "proposal", &done(&[])),
            vec!["design", "specs"]
        );
        assert!(derive_unlocks(&schema, "proposal", &done(&["proposal"])).is_empty());
        assert!(derive_unlocks(&schema, "specs", &done(&["tasks"])).is_empty());
        assert!(derive_unlocks(&schema, "proposal", &done(&["design", "specs"])).is_empty());
        assert_eq!(
            derive_unlocks(&schema, "proposal", &done(&["specs"])),
            vec!["design"]
        );
    }

    #[test]
    fn apply_state_is_blocked_for_zero_tasks_then_all_done_or_ready() {
        assert_eq!(derive_apply_state(false, true, 0, 0), ApplyState::Blocked);
        assert_eq!(derive_apply_state(false, true, 2, 0), ApplyState::AllDone);
        assert_eq!(derive_apply_state(false, true, 2, 1), ApplyState::Ready);
    }

    /// oracle 3.0.0（探測 p37／p38）：缺 required artifact 一律 blocked；schema 沒有
    /// `apply.tracks` 時不看 task，直接 ready。
    #[test]
    fn apply_state_follows_missing_artifacts_and_tracks() {
        assert_eq!(derive_apply_state(true, true, 2, 1), ApplyState::Blocked);
        assert_eq!(derive_apply_state(true, false, 0, 0), ApplyState::Blocked);
        assert_eq!(derive_apply_state(false, false, 0, 0), ApplyState::Ready);
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
