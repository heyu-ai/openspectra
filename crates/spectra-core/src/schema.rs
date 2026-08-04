//! Built-in workflow schema definitions and artifact status derivation.

#[derive(Debug, Clone, Copy)]
pub struct ArtifactDefinition {
    pub id: &'static str,
    pub output_path: &'static str,
    pub description: &'static str,
    pub deps: &'static [&'static str],
    pub instruction: &'static str,
    pub template: &'static str,
}

pub const PROPOSAL_DESCRIPTION: &str = r#"Initial proposal document outlining the change"#;

pub const PROPOSAL_INSTRUCTION: &str = r#"Create the proposal document that establishes WHY this change is needed.

Sections:
- **Why**: 1-2 sentences on the problem or opportunity. What problem does this solve? Why now?
- **What Changes**: Bullet list of changes. Be specific about new capabilities, modifications, or removals. Mark breaking changes with **BREAKING**.
- **Non-Goals** (optional): Scope exclusions and rejected approaches. If design.md will be created, Non-Goals belong there (in Goals/Non-Goals section) — leave this section empty or omit it. If design.md will be skipped, write Non-Goals here so rejected approaches and scope exclusions are recorded in a persistent artifact.
- **Capabilities**: Identify which specs will be created or modified:
  - **New Capabilities**: List capabilities being introduced. Each becomes a new `specs/<name>/spec.md`. Use kebab-case names (e.g., `user-auth`, `data-export`).
  - **Modified Capabilities**: List existing capabilities whose REQUIREMENTS are changing. Only include if spec-level behavior changes (not just implementation details). Each needs a delta spec file. Check `openspec/specs/` for existing spec names. Leave empty if no requirement changes.
- **Impact**: Affected code, APIs, dependencies, or systems.

IMPORTANT: The Capabilities section is critical. It creates the contract between
proposal and specs phases. Research existing specs before filling this in.
Each capability listed here will need a corresponding spec file.
The exact capability name becomes the `specs/<name>/` directory name —
the analyzer flags any capability without a matching spec file as Critical.

Keep it concise (1-2 pages). Focus on the "why" not the "how" -
implementation details belong in design.md.

This is the foundation - specs, design, and tasks all build on this.
"#;

pub const PROPOSAL_TEMPLATE: &str = r#"## Why

<!-- Explain the motivation for this change. What problem does this solve? Why now? -->

## What Changes

<!-- Describe what will change. Be specific about new capabilities, modifications, or removals. -->

## Non-Goals (optional)

<!-- Scope exclusions and rejected approaches. Required when design.md is skipped; optional otherwise. -->

## Capabilities

### New Capabilities

<!-- Capabilities being introduced. Replace <name> with kebab-case identifier (e.g., user-auth, data-export, api-rate-limiting). Each creates specs/<name>/spec.md -->

- `<name>`: <brief description of what this capability covers>

### Modified Capabilities

<!-- Existing capabilities whose REQUIREMENTS are changing (not just implementation).
     Only list here if spec-level behavior changes. Each needs a delta spec file.
     Use existing spec names from openspec/specs/. Leave empty if no requirement changes. -->

- `<existing-name>`: <what requirement is changing>

## Impact

<!-- Affected code, APIs, dependencies, systems -->
"#;

pub const DESIGN_DESCRIPTION: &str = r#"Technical design document with implementation details"#;

pub const DESIGN_INSTRUCTION: &str = r#"Create the design document that explains HOW to implement the change.

When to include design.md (create only if any apply):
- Cross-cutting change (multiple services/modules) or new architectural pattern
- New external dependency or significant data model changes
- Security, performance, or migration complexity
- Ambiguity that benefits from technical decisions before coding

Sections:
- **Context**: Background, current state, constraints, stakeholders
- **Goals / Non-Goals**: What this design achieves and explicitly excludes
- **Decisions**: Key technical choices with rationale (why X over Y?). Include alternatives considered for each decision.
- **Implementation Contract**: For any change that creates or modifies behavior beyond a trivial artifact-only edit, this section is REQUIRED. The contract describes the durable handoff to apply: name the observable behavior, interface or data shape, command output, error or failure mode, acceptance criteria, and explicit scope boundaries (what is in scope, what is out). The contract MUST NOT rely on source line numbers, and MUST NOT use file-path-only references as the sole way to identify required work — file paths are supporting context for behavior, never a substitute for it. Skip this section only when the change is purely artifact / documentation cleanup with no runtime, build, or tooling effect.
- **Risks / Trade-offs**: Known limitations, things that could go wrong. Format: [Risk] → Mitigation
- **Migration Plan**: Steps to deploy, rollback strategy (if applicable)
- **Open Questions**: Outstanding decisions or unknowns to resolve

Focus on architecture and approach, not line-by-line implementation.
Reference the proposal for motivation and specs for requirements.

Good design docs explain the "why" behind technical decisions.

Note: The analyzer cross-checks `###` decision headings against tasks.md.
Use descriptive heading text that will naturally appear in task descriptions.
"#;

pub const DESIGN_TEMPLATE: &str = r#"## Context

<!-- Background and current state -->

## Goals / Non-Goals

**Goals:**

<!-- What this design aims to achieve -->

**Non-Goals:**

<!-- What is explicitly out of scope -->

## Decisions

<!-- Key design decisions and rationale -->

## Implementation Contract

<!--
Required for changes that create or modify behavior. Skip only for pure
artifact / documentation cleanup with no runtime or tooling effect.

Cover the durable handoff to apply:
- Behavior: what an end user, caller, or operator observes once this change ships
- Interface / data shape: command names, function signatures, JSON shapes,
  IPC contracts, file formats — name them, do not reference line numbers
- Failure modes: error shapes, fallback behavior, what is intentionally
  silent vs. surfaced
- Acceptance criteria: how an implementer or reviewer can confirm the
  contract is satisfied (tests, CLI invocations, analyzer checks, manual
  verification)
- Scope boundaries: what is explicitly in scope and what is out — keeps
  apply from drifting into adjacent work

File paths are supporting context for locating the work; they are never
the contract itself.
-->

## Risks / Trade-offs

<!-- Known risks and trade-offs -->
"#;

pub const SPECS_DESCRIPTION: &str = r#"Detailed specifications for the change"#;

pub const SPECS_INSTRUCTION: &str = r#"Create specification files that define WHAT the system should do.

Create one spec file per capability listed in the proposal's Capabilities section.
- New capabilities: use the exact kebab-case name from the proposal (specs/<capability>/spec.md).
- Modified capabilities: use the existing spec folder name from openspec/specs/<capability>/ when creating the delta spec at specs/<capability>/spec.md.

Delta operations (use ## headers):
- **ADDED Requirements**: New capabilities
- **MODIFIED Requirements**: Changed behavior - MUST include full updated content
- **REMOVED Requirements**: Deprecated features - MUST include **Reason** and **Migration**
- **RENAMED Requirements**: Name changes only - use FROM:/TO: format

Format requirements:
- Each requirement: `### Requirement: <name>` followed by description
- Use SHALL/MUST for normative requirements. Forbidden words (analyzer flags these): should, may, might, consider, possibly, TBD, TODO, ???, TKTK — replace with SHALL/SHALL NOT/MUST/MUST NOT.
- Each scenario: `#### Scenario: <name>` with WHEN/THEN format
- **CRITICAL**: Scenarios MUST use exactly 4 hashtags (`####`). Using 3 hashtags or bullets will fail silently.
- Every requirement MUST have at least one scenario.

MODIFIED requirements workflow:
1. Locate the existing requirement in openspec/specs/<capability>/spec.md
2. Copy the ENTIRE requirement block (from `### Requirement:` through all scenarios)
3. Paste under `## MODIFIED Requirements` and edit to reflect new behavior
4. Ensure header text matches exactly (whitespace-insensitive)

Common pitfall: Using MODIFIED with partial content loses detail at archive time.
If adding new concerns without changing existing behavior, use ADDED instead.

Example:
```
## ADDED Requirements

### Requirement: User can export data
The system SHALL allow users to export their data in CSV format.

#### Scenario: Successful export
- **WHEN** user clicks "Export" button
- **THEN** system downloads a CSV file with all user data

## REMOVED Requirements

### Requirement: Legacy export
**Reason**: Replaced by new export system
**Migration**: Use new export endpoint at /api/v2/export
```

Specs should be testable - each scenario is a potential test case.

Concrete examples (SBE — Specification by Example):

Scenarios can include `##### Example: <name>` blocks (5 hashtags) with concrete
GIVEN/WHEN/THEN data that illustrates the scenario with real values:

    #### Scenario: sort by relevance
    - **WHEN** user searches
    - **THEN** results appear sorted by score

    ##### Example: three items sorted
    - **GIVEN** items: A(score=0.9), B(score=0.3), C(score=0.7)
    - **WHEN** user searches for "test"
    - **THEN** results appear in order: A, C, B

For multiple test cases, use a table inside the example block:

    ##### Example: boundary cases
    | Input | Expected | Notes |
    |-------|----------|-------|
    | "" | error: empty query | empty string |
    | "a" | minimum 2 chars warning | too short |
    | "valid query" | results returned | normal case |

When to add examples:
- The scenario involves computed output (sorting, filtering, scoring, ranking)
- The scenario involves state transitions or data transformation
- The boundary behavior is non-obvious
- A table can replace 3+ separate scenarios

When to skip examples:
- Simple UI navigation flows (click button, see page)
- Straightforward CRUD with no computed logic
- The WHEN/THEN already contains concrete values

Examples are optional — the analyzer will suggest adding them for abstract scenarios
but will not block specs without examples.

Spec files MUST always be written in English regardless of project locale settings,
because they use normative language (SHALL/MUST/WHEN/THEN).
"#;

pub const SPECS_TEMPLATE: &str = r#"## ADDED Requirements

### Requirement: <!-- requirement name -->

<!-- requirement text -->

#### Scenario: <!-- scenario name -->

- **WHEN** <!-- condition -->
- **THEN** <!-- expected outcome -->

<!-- Optional: add concrete examples when the scenario involves data transformation,
     ordering, filtering, scoring, or state transitions. Skip for simple UI flows. -->

##### Example: <!-- example name (optional) -->

- **GIVEN** <!-- concrete test data with real values -->
- **WHEN** <!-- specific input -->
- **THEN** <!-- specific expected output -->

<!-- For multiple test cases, use a table instead:

##### Example: <!-- example name -->

| Input | Expected Output | Notes |
| ----- | --------------- | ----- |
| ...   | ...             | ...   |

-->
"#;

pub const TASKS_DESCRIPTION: &str = r#"Implementation checklist with trackable tasks"#;

pub const TASKS_INSTRUCTION: &str = r#"Create the task list that breaks down the implementation work.

**IMPORTANT: Follow the template below exactly.** The apply phase parses
checkbox format to track progress. Tasks not using `- [ ]` won't be tracked.

Guidelines:
- Group related tasks under ## numbered headings
- Each task MUST be a checkbox: `- [ ] X.Y Task description`
- Use the instructions JSON `locale` for human-readable task group headings
  and task descriptions.
- Preserve machine-readable syntax and technical tokens exactly: markdown
  headings, checkbox markers, task numbers, `[P]` markers, file paths,
  symbol names, commands, API names, and code identifiers MUST NOT be
  translated or localized.
- Tasks should be small enough to complete in one session
- Order tasks by dependency (what must be done first?)
- **Behavior + verification (REQUIRED for every non-trivial task):**
  - Each task MUST state the behavior or contract being delivered — what is
    observably true when the task is complete (user-visible behavior, generated
    artifact contract, CLI/IPC output, or tool behavior). "Edit file X" is
    NOT a behavior; it is supporting context for locating the work.
  - Each task MUST also state how completion is verified — a test name, a
    CLI invocation, an analyzer check, a manual assertion, or a content
    review on a generated artifact. A task without a verification target
    is not a valid task.
  - File paths MAY appear in a task description, but only as locator
    context. The task SHALL still state the behavior or contract on top
    of any file path it mentions.
  - File-edit-only tasks (e.g. "Update file X to handle Y") are invalid
    unless they also describe the resulting behavior and how it is
    verified.
- Cross-referencing (analyzer checks these):
  - Every `### Requirement:` name from specs MUST appear as a case-insensitive substring in at least one task description
  - If design.md exists, every `###` heading from design.md should be referenced in at least one task description

Example:
```
## 1. Setup

- [ ] 1.1 Create new module structure
- [ ] 1.2 Add dependencies to package.json

## 2. Core Implementation

- [ ] 2.1 Implement data export function
- [ ] 2.2 Add CSV formatting utilities
```

Reference specs for what needs to be built. If design.md exists, reference it for how to build it.
Each task should be verifiable - you know when it's done.
"#;

pub const TASKS_TEMPLATE: &str = r#"<!--
Each task description MUST state:
- the behavior or contract being delivered (what is observably true when the
  task is complete), and
- the verification target that proves completion (test, CLI invocation,
  analyzer check, manual assertion, or content review).

File paths are supporting context for locating the work, never the task
itself. "Edit file X" is not a valid task — it is missing both behavior and
verification.
-->

## 1. <!-- Task Group Name -->

- [ ] 1.1 <!-- Behavior/contract delivered + verification target -->
- [ ] 1.2 <!-- Behavior/contract delivered + verification target -->

## 2. <!-- Task Group Name -->

- [ ] 2.1 <!-- Behavior/contract delivered + verification target -->
- [ ] 2.2 <!-- Behavior/contract delivered + verification target -->
"#;

pub const ARTIFACTS: [ArtifactDefinition; 4] = [
    ArtifactDefinition {
        id: "proposal",
        output_path: "proposal.md",
        description: PROPOSAL_DESCRIPTION,
        deps: &[],
        instruction: PROPOSAL_INSTRUCTION,
        template: PROPOSAL_TEMPLATE,
    },
    ArtifactDefinition {
        id: "design",
        output_path: "design.md",
        description: DESIGN_DESCRIPTION,
        deps: &["proposal"],
        instruction: DESIGN_INSTRUCTION,
        template: DESIGN_TEMPLATE,
    },
    ArtifactDefinition {
        id: "specs",
        output_path: "specs/**/*.md",
        description: SPECS_DESCRIPTION,
        deps: &["proposal"],
        instruction: SPECS_INSTRUCTION,
        template: SPECS_TEMPLATE,
    },
    ArtifactDefinition {
        id: "tasks",
        output_path: "tasks.md",
        description: TASKS_DESCRIPTION,
        deps: &["specs"],
        instruction: TASKS_INSTRUCTION,
        template: TASKS_TEMPLATE,
    },
];

pub fn artifacts() -> &'static [ArtifactDefinition] {
    &ARTIFACTS
}

pub const SCHEMA_NAME: &str = "spec-driven";
pub const APPLY_REQUIRES: &[&str] = &["tasks"];

/// Source label the oracle reports for the built-in schema in
/// `spectra schemas`. `spec-driven` ships inside the binary ("package"), as
/// opposed to project- or user-level schema files (which OpenSpectra does not
/// support). Pinned against the oracle — see docs/reverse-engineering/schemas.md.
pub const SCHEMA_SOURCE: &str = "package";

/// One-line schema description shown by `spectra schemas`. Pinned byte-for-byte
/// against the oracle. The `→`-separated list is the recommended *authoring*
/// order (proposal → specs → design → tasks), which deliberately differs from
/// `ARTIFACTS`' dependency-definition order (proposal, design, specs, tasks).
pub const SCHEMA_DESCRIPTION: &str =
    "Default OpenSpec workflow - proposal → specs → design → tasks";

/// Artifact ids in the workflow order the oracle lists for `spectra schemas`
/// (proposal → specs → design → tasks). Kept as its own constant — not derived
/// from `ARTIFACTS` — because the listing mirrors the authoring sequence
/// encoded in `SCHEMA_DESCRIPTION`, not the artifact-definition order. A unit
/// test asserts this stays a permutation of the `ARTIFACTS` id set so a future
/// artifact addition can't silently desync the two.
pub const SCHEMA_ARTIFACT_ORDER: &[&str] = &["proposal", "specs", "design", "tasks"];

/// A workflow schema as reported by `spectra schemas`. Field declaration order
/// matches the oracle's `--json` output (alphabetical: artifacts, description,
/// name, source), so `serde` serializes it byte-for-byte against the golden
/// without needing `serde_json::Value`'s BTreeMap sorting.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SchemaListing {
    pub artifacts: &'static [&'static str],
    pub description: &'static str,
    pub name: &'static str,
    pub source: &'static str,
}

/// The built-in workflow schema registry backing `spectra schemas`.
/// OpenSpectra ships exactly one schema (`spec-driven`); project/user schema
/// discovery is not implemented, so this always returns a single entry.
pub fn schemas() -> Vec<SchemaListing> {
    vec![SchemaListing {
        artifacts: SCHEMA_ARTIFACT_ORDER,
        description: SCHEMA_DESCRIPTION,
        name: SCHEMA_NAME,
        source: SCHEMA_SOURCE,
    }]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactState {
    Done,
    Ready,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactStatus {
    pub id: &'static str,
    pub output_path: &'static str,
    pub status: ArtifactState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing_deps: Option<Vec<&'static str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    pub change_name: String,
    pub schema_name: &'static str,
    pub is_complete: bool,
    pub apply_requires: &'static [&'static str],
    pub artifacts: Vec<ArtifactStatus>,
}

fn artifact_status(
    artifact: &'static ArtifactDefinition,
    done_ids: &std::collections::HashSet<&'static str>,
) -> ArtifactStatus {
    let missing_deps: Vec<_> = artifact
        .deps
        .iter()
        .copied()
        .filter(|dep| !done_ids.contains(dep))
        .collect();
    let (status, missing_deps) = if done_ids.contains(artifact.id) {
        (ArtifactState::Done, None)
    } else if missing_deps.is_empty() {
        (ArtifactState::Ready, None)
    } else {
        (ArtifactState::Blocked, Some(missing_deps))
    };

    ArtifactStatus {
        id: artifact.id,
        output_path: artifact.output_path,
        status,
        missing_deps,
    }
}

pub fn artifact_done(
    artifact: &ArtifactDefinition,
    change_dir: &std::path::Path,
) -> anyhow::Result<bool> {
    // A globbed output path ("specs/**/*.md") marks a directory-shaped
    // artifact: done = at least one .md anywhere under the glob's root
    // directory. Derived from the data instead of matching a hardcoded id so
    // a future directory-shaped artifact doesn't have to be named "specs".
    if artifact.output_path.contains('*') {
        let root = artifact
            .output_path
            .split('/')
            .next()
            .expect("split always yields at least one segment");
        let mut visited = std::collections::HashSet::new();
        return contains_markdown_file(&change_dir.join(root), &mut visited);
    }
    Ok(change_dir.join(artifact.output_path).is_file())
}

/// Recursive "any .md file under this directory" scan backing the specs
/// done-check.
///
/// Semantics probed against Spectra.app 2.3.1 (see
/// docs/reverse-engineering/artifact-workflow.md): the oracle's
/// `specs/**/*.md` glob FOLLOWS directory symlinks — a change whose specs
/// live behind a symlink still counts as done — so this walk resolves
/// symlinks via `fs::metadata` (unlike `fsutil::collect_delta_specs`, whose
/// archive/validate domain deliberately skips them). `visited` holds
/// canonicalized directories to make symlink cycles terminate instead of
/// recursing forever.
///
/// Error semantics are match-wins and ORDER-INDEPENDENT: a found `.md`
/// anywhere means done, regardless of unreadable siblings (a glob that finds
/// a match found it; an unreadable directory can't hide it). Only when NO
/// match is found does a recorded traversal error propagate — an unreadable
/// specs tree must not be silently misreported as "not done" (matching the
/// `change::try_load` convention). NotFound (specs/ not created yet, or a
/// dangling symlink) is the normal "not done" case, never an error.
fn contains_markdown_file(
    dir: &std::path::Path,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
) -> anyhow::Result<bool> {
    let mut first_error = None;
    if scan_for_markdown(dir, visited, &mut first_error) {
        return Ok(true);
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(false),
    }
}

/// DFS worker for [`contains_markdown_file`]: returns whether a `.md` was
/// found; traversal errors are recorded into `first_error` (first one wins)
/// instead of aborting, so the scan visits every reachable entry before the
/// caller decides between "not done" and error.
fn scan_for_markdown(
    dir: &std::path::Path,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
    first_error: &mut Option<anyhow::Error>,
) -> bool {
    fn record(first_error: &mut Option<anyhow::Error>, error: anyhow::Error) {
        if first_error.is_none() {
            *first_error = Some(error);
        }
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return false,
        Err(e) => {
            record(
                first_error,
                anyhow::Error::new(e).context(format!("reading specs directory {}", dir.display())),
            );
            return false;
        }
    };
    match std::fs::canonicalize(dir) {
        Ok(canonical) => {
            if !visited.insert(canonical) {
                return false;
            }
        }
        Err(e) => {
            record(
                first_error,
                anyhow::Error::new(e)
                    .context(format!("resolving specs directory {}", dir.display())),
            );
            return false;
        }
    }

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                record(
                    first_error,
                    anyhow::Error::new(e)
                        .context(format!("reading specs directory {}", dir.display())),
                );
                continue;
            }
        };
        let path = entry.path();
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            // Dangling symlink: nothing behind it to count.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                record(
                    first_error,
                    anyhow::Error::new(e)
                        .context(format!("reading specs entry {}", path.display())),
                );
                continue;
            }
        };
        if metadata.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            return true;
        }
        if metadata.is_dir() && scan_for_markdown(&path, visited, first_error) {
            return true;
        }
    }
    false
}

pub fn derive_status(
    change_name: &str,
    change_dir: &std::path::Path,
) -> anyhow::Result<StatusReport> {
    let mut done_ids = std::collections::HashSet::new();
    for artifact in ARTIFACTS.iter() {
        if artifact_done(artifact, change_dir)? {
            done_ids.insert(artifact.id);
        }
    }
    let artifacts = ARTIFACTS
        .iter()
        .map(|artifact| artifact_status(artifact, &done_ids))
        .collect();

    Ok(StatusReport {
        change_name: change_name.to_string(),
        schema_name: SCHEMA_NAME,
        is_complete: done_ids.len() == ARTIFACTS.len(),
        apply_requires: APPLY_REQUIRES,
        artifacts,
    })
}

/// The `schema:` selector in `<spec_dir>/config.yaml` — the *project-level*
/// default, consulted only when the change records no schema of its own.
///
/// `None` when the file is absent, has no `schema` key, or its value is blank
/// (probed: a project `config.yaml` holding a bare `schema:` runs the built-in
/// workflow and exits 0, unlike the change-level key — see
/// [`require_supported`]). A read or parse failure is also `None`: an unreadable
/// selector must not be louder than a missing one, since the built-in schema is
/// the default either way.
///
/// Caveat on "unparseable": `serde_yaml` stringifies scalar types, so
/// `schema: 123` yields `Some("123")` rather than `None`. Only a structural
/// mismatch (sequence/map) or a syntax error reaches the `None` path. That is
/// harmless here — `"123"` is not the built-in name, so `require_supported`
/// still fails loud — but the value is a coerced scalar, not "unset".
pub fn configured_schema_name(cfg: &crate::Config) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct SpecConfig {
        schema: Option<String>,
    }
    let text = std::fs::read_to_string(cfg.root.join(&cfg.spec_dir).join("config.yaml")).ok()?;
    let parsed: SpecConfig = serde_yaml::from_str(&text).ok()?;
    parsed.schema.filter(|s| !s.trim().is_empty())
}

/// Resolve which schema a command should run under and reject anything but the
/// built-in.
///
/// Resolution order, probed against the v2.3.1 oracle — the **change's own
/// `.openspec.yaml` dominates the project config**, which is the layer an
/// earlier revision of this function missed:
///
/// 1. `--schema` when given (overrides everything, including a change whose
///    recorded schema resolves nowhere).
/// 2. the change's `.openspec.yaml` `schema:`. Probed: with the change
///    recording `spec-driven`, a project `config.yaml` naming an unknown schema
///    is **ignored** and `status` exits 0; with the two swapped, `status`
///    exits 1. Since `spectra new change` stamps this key, it is set on
///    essentially every real change.
/// 3. `<spec_dir>/config.yaml`'s `schema:`, used only when the change records
///    none (a hand-made change directory, or one predating the metadata file).
/// 4. otherwise the built-in.
///
/// Callers must resolve and load the change **before** calling this: the oracle
/// reports `Change 'X' not found.` ahead of any schema error (probed).
///
/// Errors on anything but the built-in. OpenSpectra cannot load a custom schema
/// yet (#126), and *silently* falling back to `spec-driven` was worse than
/// failing: the project's own artifact instructions vanished from
/// `instructions` output with exit 0 and no warning (#117).
///
/// One edge case is deliberately not reproduced: an explicit `schema:` with a
/// null/blank value **in the change's** `.openspec.yaml` makes the oracle report
/// `Schema '' not found`, whereas `ChangeMetadata`'s `Option<String>` cannot
/// tell that from an absent key, so OpenSpectra falls through to the project
/// config. Matching it would need `Option<Option<String>>` on a struct that is
/// round-tripped by `archive`, for an input no tool writes.
pub fn require_supported(
    cfg: &crate::Config,
    explicit: Option<&str>,
    change: Option<&crate::change::Change>,
) -> anyhow::Result<()> {
    let resolved;
    let name = match explicit {
        Some(name) => name,
        None => {
            let from_change = change.and_then(|c| c.metadata.schema.clone());
            match from_change.or_else(|| configured_schema_name(cfg)) {
                Some(name) => {
                    resolved = name;
                    resolved.as_str()
                }
                None => return Ok(()),
            }
        }
    };
    if name == SCHEMA_NAME {
        return Ok(());
    }
    let definition = cfg
        .root
        .join(&cfg.spec_dir)
        .join("schemas")
        .join(name)
        .join("schema.yaml");
    if definition.is_file() {
        return Err(anyhow::anyhow!(
            "Schema '{name}' is defined at {} but OpenSpectra can only run the built-in \
             '{SCHEMA_NAME}' schema; loading custom schemas is tracked by issue #126. \
             Set 'schema: {SCHEMA_NAME}' in {}/config.yaml to proceed with the built-in workflow.",
            definition.display(),
            cfg.spec_dir
        ));
    }
    // The oracle's wording, byte for byte, for a name that resolves nowhere.
    Err(anyhow::anyhow!(
        "Schema not found: Schema '{name}' not found in project, user, or built-in locations"
    ))
}

pub fn status(
    cfg: &crate::Config,
    explicit_change: Option<&str>,
    schema_name: Option<&str>,
) -> anyhow::Result<StatusReport> {
    // Change resolution first: the oracle reports `Change 'X' not found.`
    // ahead of any schema error, and the change's own `.openspec.yaml` is the
    // dominant schema selector (probed) — so the gate needs the loaded change.
    let change_name = crate::change::resolve(cfg, explicit_change)?;
    let change = crate::change::try_load(cfg, &change_name)?
        .ok_or_else(|| anyhow::anyhow!("Change '{change_name}' not found."))?;
    require_supported(cfg, schema_name, Some(&change))?;
    derive_status(&change.name, &change.dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn project(label: &str) -> (TempDir, crate::Config) {
        let tmp = TempDir::new(label);
        let cfg = crate::Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        std::fs::create_dir_all(tmp.join("openspec")).unwrap();
        (tmp, cfg)
    }

    fn write_spec_config(cfg: &crate::Config, body: &str) {
        std::fs::write(cfg.root.join(&cfg.spec_dir).join("config.yaml"), body).unwrap();
    }

    /// A change whose `.openspec.yaml` records `schema`, for the dominant
    /// resolution layer. Only `metadata.schema` is read by `require_supported`,
    /// so the other fields are placeholders.
    fn change_with_schema(schema: Option<&str>) -> crate::change::Change {
        crate::change::Change {
            name: "c1".to_string(),
            dir: std::path::PathBuf::from("openspec/changes/c1"),
            metadata: crate::change::ChangeMetadata {
                schema: schema.map(str::to_string),
                ..Default::default()
            },
            started_sha: None,
            parked: false,
        }
    }

    #[test]
    fn configured_schema_name_reads_the_spec_dir_config() {
        let (_tmp, cfg) = project("schema-configured");
        assert_eq!(configured_schema_name(&cfg), None);

        write_spec_config(&cfg, "schema: mycustom\n# a comment\n");
        assert_eq!(configured_schema_name(&cfg), Some("mycustom".to_string()));
    }

    #[test]
    fn configured_schema_name_ignores_blank_and_unparseable_config() {
        let (_tmp, cfg) = project("schema-configured-blank");

        write_spec_config(&cfg, "schema: \"   \"\n");
        assert_eq!(configured_schema_name(&cfg), None);

        write_spec_config(&cfg, "schema: [not, a, string\n");
        assert_eq!(configured_schema_name(&cfg), None);
    }

    #[test]
    fn require_supported_accepts_the_builtin_and_an_unconfigured_project() {
        let (_tmp, cfg) = project("schema-supported-ok");
        assert!(require_supported(&cfg, None, None).is_ok());

        write_spec_config(&cfg, "schema: spec-driven\n");
        assert!(require_supported(&cfg, None, None).is_ok());
        assert!(require_supported(&cfg, Some(SCHEMA_NAME), None).is_ok());
    }

    #[test]
    fn require_supported_uses_the_oracle_wording_for_a_schema_that_resolves_nowhere() {
        let (_tmp, cfg) = project("schema-supported-missing");
        write_spec_config(&cfg, "schema: no-such-schema\n");

        assert_eq!(
            require_supported(&cfg, None, None).unwrap_err().to_string(),
            "Schema not found: Schema 'no-such-schema' not found in project, user, \
             or built-in locations"
        );
    }

    #[test]
    fn require_supported_reports_a_present_custom_schema_as_unsupported_not_missing() {
        // Claiming a schema is "not found in project ... locations" while it
        // sits in the project would be a false statement, so this case gets
        // its own message.
        let (_tmp, cfg) = project("schema-supported-present");
        write_spec_config(&cfg, "schema: mycustom\n");
        let definition = cfg
            .root
            .join("openspec")
            .join("schemas")
            .join("mycustom")
            .join("schema.yaml");
        std::fs::create_dir_all(definition.parent().unwrap()).unwrap();
        std::fs::write(&definition, "name: mycustom\nartifacts: []\n").unwrap();

        let err = require_supported(&cfg, None, None).unwrap_err().to_string();
        assert!(err.contains(&definition.display().to_string()), "{err}");
        assert!(err.contains("#126"), "{err}");
        assert!(!err.contains("not found"), "{err}");
    }

    #[test]
    fn an_explicit_schema_flag_overrides_the_configured_one() {
        let (_tmp, cfg) = project("schema-flag-wins");
        write_spec_config(&cfg, "schema: no-such-schema\n");

        assert!(require_supported(&cfg, Some(SCHEMA_NAME), None).is_ok());
        assert!(require_supported(&cfg, Some("another-missing"), None).is_err());
    }

    #[test]
    fn the_changes_own_schema_outranks_the_project_config() {
        // The layer an earlier revision missed. Probed on v2.3.1: a change
        // recording `spec-driven` runs fine even when the project config names
        // an unknown schema, and `spectra new change` stamps that key -- so
        // reading only config.yaml hard-failed changes the oracle accepts.
        let (_tmp, cfg) = project("schema-change-wins");
        write_spec_config(&cfg, "schema: no-such-schema\n");

        let builtin_change = change_with_schema(Some(SCHEMA_NAME));
        assert!(
            require_supported(&cfg, None, Some(&builtin_change)).is_ok(),
            "a change recording the built-in schema must not be blocked by an \
             unrelated project config"
        );

        // ...and the reverse: the change's own bad name blocks even with a
        // healthy project config.
        write_spec_config(&cfg, "schema: spec-driven\n");
        let custom_change = change_with_schema(Some("no-such-schema"));
        assert_eq!(
            require_supported(&cfg, None, Some(&custom_change))
                .unwrap_err()
                .to_string(),
            "Schema not found: Schema 'no-such-schema' not found in project, user, \
             or built-in locations"
        );
    }

    #[test]
    fn the_project_config_applies_only_when_the_change_records_no_schema() {
        let (_tmp, cfg) = project("schema-change-absent");
        write_spec_config(&cfg, "schema: no-such-schema\n");

        let no_metadata_schema = change_with_schema(None);
        assert!(
            require_supported(&cfg, None, Some(&no_metadata_schema)).is_err(),
            "with no change-level schema the project config is the selector"
        );
    }

    #[test]
    fn an_explicit_flag_overrides_even_the_changes_own_schema() {
        let (_tmp, cfg) = project("schema-flag-beats-change");
        let custom_change = change_with_schema(Some("no-such-schema"));

        assert!(require_supported(&cfg, Some(SCHEMA_NAME), Some(&custom_change)).is_ok());
    }

    #[test]
    fn spec_driven_artifacts_have_canonical_order_paths_and_dependencies() {
        let artifacts = artifacts();

        assert_eq!(artifacts.len(), 4);
        assert_eq!(
            artifacts
                .iter()
                .map(|artifact| (artifact.id, artifact.output_path, artifact.deps))
                .collect::<Vec<_>>(),
            vec![
                ("proposal", "proposal.md", &[][..]),
                ("design", "design.md", &["proposal"][..]),
                ("specs", "specs/**/*.md", &["proposal"][..]),
                ("tasks", "tasks.md", &["specs"][..]),
            ]
        );
    }

    #[test]
    fn embedded_instruction_text_matches_oracle_goldens_byte_for_byte() {
        let golden_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/reverse-engineering/golden");

        for artifact in artifacts() {
            let path = golden_dir.join(format!("instructions-{}-2.3.1.json", artifact.id));
            let text = std::fs::read_to_string(&path).unwrap();
            let golden: serde_json::Value = serde_json::from_str(&text).unwrap();

            assert_eq!(
                artifact.description,
                golden["description"].as_str().unwrap()
            );
            assert_eq!(
                artifact.instruction,
                golden["instruction"].as_str().unwrap()
            );
            assert_eq!(artifact.template, golden["template"].as_str().unwrap());
            // Structural fields, previously pinned only by an inline
            // re-declaration (self-referential): compare against the oracle
            // capture directly.
            assert_eq!(artifact.output_path, golden["outputPath"].as_str().unwrap());
            let golden_deps: Vec<&str> = golden["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|dep| dep["id"].as_str().unwrap())
                .collect();
            assert_eq!(artifact.deps, golden_deps.as_slice());
        }
    }

    #[test]
    fn schemas_registry_matches_oracle_golden_shape() {
        let listings = schemas();
        assert_eq!(listings.len(), 1);
        let schema = &listings[0];
        assert_eq!(schema.name, "spec-driven");
        assert_eq!(schema.source, "package");
        assert_eq!(
            schema.description,
            "Default OpenSpec workflow - proposal → specs → design → tasks"
        );
        assert_eq!(schema.artifacts, &["proposal", "specs", "design", "tasks"]);
    }

    #[test]
    fn schemas_json_serializes_byte_for_byte_against_the_oracle_golden() {
        let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/reverse-engineering/golden/schemas-2.3.1.json");
        let golden = std::fs::read_to_string(&golden_path).unwrap();

        let rendered = serde_json::to_string_pretty(&schemas()).unwrap();

        // The golden captures the oracle's `--json` stdout, which ends in a
        // trailing newline the CLI adds via `println!`; the serializer itself
        // emits no trailing newline, so compare against the trimmed golden.
        assert_eq!(rendered, golden.trim_end());
    }

    #[test]
    fn schema_artifact_order_is_a_permutation_of_the_artifact_definition_ids() {
        // The listing order is maintained by hand (authoring order, not
        // dependency order); this guards against a future ARTIFACTS change
        // that adds/removes an artifact without updating SCHEMA_ARTIFACT_ORDER.
        let mut listed: Vec<&str> = SCHEMA_ARTIFACT_ORDER.to_vec();
        let mut defined: Vec<&str> = ARTIFACTS.iter().map(|artifact| artifact.id).collect();
        listed.sort_unstable();
        defined.sort_unstable();
        assert_eq!(listed, defined);
    }

    #[test]
    fn schema_description_ends_with_the_artifact_order_flow() {
        // The permutation test above ties SCHEMA_ARTIFACT_ORDER to ARTIFACTS,
        // but the human-readable SCHEMA_DESCRIPTION also spells out that flow
        // (`... - proposal → specs → design → tasks`). Without this, a future
        // artifact could update SCHEMA_ARTIFACT_ORDER + the JSON golden while
        // leaving the description (and text golden) silently stale. Pin that
        // the description's trailing flow equals the ordered artifact list.
        let flow = SCHEMA_ARTIFACT_ORDER.join(" → ");
        assert!(
            SCHEMA_DESCRIPTION.ends_with(&flow),
            "SCHEMA_DESCRIPTION {SCHEMA_DESCRIPTION:?} must end with the artifact flow {flow:?}"
        );
    }

    #[test]
    fn empty_change_has_only_proposal_ready() {
        let change_dir = TempDir::new("empty");

        let report = derive_status("demo-feature", &change_dir).unwrap();

        assert!(!report.is_complete);
        assert_eq!(report.apply_requires, &["tasks"]);
        assert_eq!(report.artifacts[0].status, ArtifactState::Ready);
        assert_eq!(report.artifacts[0].missing_deps, None);
        assert_eq!(report.artifacts[1].status, ArtifactState::Blocked);
        assert_eq!(report.artifacts[1].missing_deps, Some(vec!["proposal"]));
        assert_eq!(report.artifacts[2].status, ArtifactState::Blocked);
        assert_eq!(report.artifacts[2].missing_deps, Some(vec!["proposal"]));
        assert_eq!(report.artifacts[3].status, ArtifactState::Blocked);
        assert_eq!(report.artifacts[3].missing_deps, Some(vec!["specs"]));
    }

    #[test]
    fn proposal_file_makes_its_direct_dependents_ready() {
        let change_dir = TempDir::new("partial");
        std::fs::write(change_dir.join("proposal.md"), "# Proposal\n").unwrap();

        let report = derive_status("demo-feature", &change_dir).unwrap();

        assert_eq!(report.artifacts[0].status, ArtifactState::Done);
        assert_eq!(report.artifacts[1].status, ArtifactState::Ready);
        assert_eq!(report.artifacts[2].status, ArtifactState::Ready);
        assert_eq!(report.artifacts[3].status, ArtifactState::Blocked);
        assert_eq!(report.artifacts[3].missing_deps, Some(vec!["specs"]));
    }

    #[test]
    fn all_artifact_files_make_the_change_complete() {
        let change_dir = TempDir::new("complete");
        std::fs::write(change_dir.join("proposal.md"), "# Proposal\n").unwrap();
        std::fs::write(change_dir.join("design.md"), "# Design\n").unwrap();
        std::fs::write(change_dir.join("tasks.md"), "# Tasks\n").unwrap();
        let nested_specs = change_dir.join("specs").join("billing").join("invoices");
        std::fs::create_dir_all(&nested_specs).unwrap();
        std::fs::write(nested_specs.join("spec.md"), "# Spec\n").unwrap();

        let report = derive_status("demo-feature", &change_dir).unwrap();

        assert!(report.is_complete);
        assert!(report
            .artifacts
            .iter()
            .all(|artifact| artifact.status == ArtifactState::Done));
        assert!(report
            .artifacts
            .iter()
            .all(|artifact| artifact.missing_deps.is_none()));
    }

    #[test]
    fn deleting_specs_does_not_cascade_to_a_done_tasks_artifact() {
        let change_dir = TempDir::new("delete-specs");
        std::fs::write(change_dir.join("proposal.md"), "# Proposal\n").unwrap();
        std::fs::write(change_dir.join("design.md"), "# Design\n").unwrap();
        std::fs::write(change_dir.join("tasks.md"), "# Tasks\n").unwrap();
        let specs = change_dir.join("specs").join("billing");
        std::fs::create_dir_all(&specs).unwrap();
        std::fs::write(specs.join("spec.md"), "# Spec\n").unwrap();
        std::fs::remove_dir_all(change_dir.join("specs")).unwrap();

        let report = derive_status("demo-feature", &change_dir).unwrap();

        assert!(!report.is_complete);
        assert_eq!(report.artifacts[2].status, ArtifactState::Ready);
        assert_eq!(report.artifacts[3].status, ArtifactState::Done);
    }

    #[test]
    fn non_markdown_files_under_specs_do_not_count_as_done() {
        let change_dir = TempDir::new("specs-non-md");
        std::fs::write(change_dir.join("proposal.md"), "# Proposal\n").unwrap();
        let sub = change_dir.join("specs").join("notes");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("notes.txt"), "scratch\n").unwrap();

        let report = derive_status("demo-feature", &change_dir).unwrap();

        assert_eq!(report.artifacts[2].status, ArtifactState::Ready);
    }

    #[test]
    #[cfg(unix)]
    fn specs_behind_a_directory_symlink_count_as_done() {
        // Probed against Spectra.app 2.3.1 (2026-07-18): the oracle's
        // specs/**/*.md glob follows directory symlinks -- status reports
        // specs "done" when specs/<cap> is a symlink to a directory that
        // contains a spec.md (see docs/reverse-engineering/artifact-workflow.md).
        let change_dir = TempDir::new("symlink-specs");
        let outside = TempDir::new("symlink-specs-target");
        std::fs::write(outside.join("spec.md"), "# Spec\n").unwrap();
        let specs = change_dir.join("specs");
        std::fs::create_dir_all(&specs).unwrap();
        std::os::unix::fs::symlink(&*outside, specs.join("cap1")).unwrap();

        let report = derive_status("demo-feature", &change_dir).unwrap();

        assert_eq!(report.artifacts[2].status, ArtifactState::Done);
    }

    #[test]
    #[cfg(unix)]
    fn symlink_cycle_in_specs_terminates_without_matches() {
        let change_dir = TempDir::new("symlink-cycle");
        std::fs::write(change_dir.join("proposal.md"), "# Proposal\n").unwrap();
        let specs = change_dir.join("specs");
        std::fs::create_dir_all(specs.join("sub")).unwrap();
        std::os::unix::fs::symlink(&specs, specs.join("sub").join("loop")).unwrap();

        let report = derive_status("demo-feature", &change_dir).unwrap();

        assert_eq!(report.artifacts[2].status, ArtifactState::Ready);
    }

    #[test]
    #[cfg(unix)]
    fn found_markdown_wins_over_an_unreadable_sibling_directory() {
        // Match-wins determinism: with a valid spec.md AND an unreadable
        // sibling dir in the same specs tree, the result is "done" regardless
        // of filesystem iteration order (the scan records the error but keeps
        // walking; a found match discards it).
        use std::os::unix::fs::PermissionsExt;
        let change_dir = TempDir::new("specs-mixed");
        std::fs::write(change_dir.join("proposal.md"), "# Proposal\n").unwrap();
        let specs = change_dir.join("specs");
        std::fs::create_dir_all(specs.join("locked")).unwrap();
        std::fs::create_dir_all(specs.join("open")).unwrap();
        std::fs::write(specs.join("open").join("spec.md"), "# Spec\n").unwrap();
        std::fs::set_permissions(specs.join("locked"), std::fs::Permissions::from_mode(0o000))
            .unwrap();

        let result = derive_status("demo-feature", &change_dir);

        std::fs::set_permissions(specs.join("locked"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let report = result.unwrap();
        assert_eq!(report.artifacts[2].status, ArtifactState::Done);
    }

    #[test]
    #[cfg(unix)]
    fn unreadable_specs_directory_is_an_error_not_not_done() {
        use std::os::unix::fs::PermissionsExt;
        let change_dir = TempDir::new("specs-unreadable");
        let specs = change_dir.join("specs");
        std::fs::create_dir_all(&specs).unwrap();
        std::fs::set_permissions(&specs, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = derive_status("demo-feature", &change_dir);

        // Restore before asserting so TempDir's Drop can clean up.
        std::fs::set_permissions(&specs, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            result.is_err(),
            "an unreadable specs/ must propagate, not silently read as not-done"
        );
    }

    #[test]
    fn missing_dependencies_preserve_schema_order() {
        const MULTI_DEP_ARTIFACT: ArtifactDefinition = ArtifactDefinition {
            id: "example",
            output_path: "example.md",
            description: "",
            deps: &["proposal", "design", "specs"],
            instruction: "",
            template: "",
        };
        let done_ids = std::collections::HashSet::from(["design"]);

        let status = artifact_status(&MULTI_DEP_ARTIFACT, &done_ids);

        assert_eq!(status.status, ArtifactState::Blocked);
        assert_eq!(status.missing_deps, Some(vec!["proposal", "specs"]));
    }

    #[test]
    fn status_rejects_an_unknown_schema_with_oracle_error() {
        // Change resolution precedes the schema gate, so an unknown schema
        // needs a resolvable change to surface at all. Probed on v2.3.1: in a
        // project with no changes, `status --schema bogus` prints
        // `No active changes.` and never reaches the schema error -- an earlier
        // revision of this test asserted the opposite order.
        let root = TempDir::new("unknown-schema");
        let cfg = crate::Config {
            root: root.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        std::fs::create_dir_all(cfg.changes_dir().join("c1")).unwrap();
        std::fs::write(cfg.changes_dir().join("c1").join("proposal.md"), "# p\n").unwrap();

        let err = status(&cfg, None, Some("bogus")).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Schema not found: Schema 'bogus' not found in project, user, or built-in locations"
        );
    }

    #[test]
    fn status_reports_a_missing_change_before_an_unknown_schema() {
        // The order itself, pinned: probed, the oracle emits
        // `Change 'X' not found.` ahead of any schema error.
        let root = TempDir::new("order-change-first");
        let cfg = crate::Config {
            root: root.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        let err = status(&cfg, Some("ghost"), Some("bogus")).unwrap_err();

        assert_eq!(err.to_string(), "Change 'ghost' not found.");
    }

    #[test]
    fn status_rejects_an_explicit_missing_change_with_oracle_error() {
        let root = TempDir::new("missing-change");
        let cfg = crate::Config {
            root: root.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        let err = status(&cfg, Some("nope"), None).unwrap_err();

        assert_eq!(err.to_string(), "Change 'nope' not found.");
    }
}
