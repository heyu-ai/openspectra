//! Workflow artifact scaffolding and stdin-content validation.

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;

#[derive(Debug)]
pub struct NewArtifactOutcome {
    pub artifact: &'static str,
    pub change: String,
    pub path: PathBuf,
    pub validated: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactType {
    Proposal,
    Design,
    Tasks,
    Spec,
}

impl ArtifactType {
    fn parse(type_name: &str) -> Result<Self> {
        match type_name {
            "proposal" => Ok(Self::Proposal),
            "design" => Ok(Self::Design),
            "tasks" => Ok(Self::Tasks),
            "spec" => Ok(Self::Spec),
            _ => Err(anyhow!(
                "Unknown artifact type '{type_name}'. Valid types: proposal, design, tasks, spec"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Proposal => "proposal",
            Self::Design => "design",
            Self::Tasks => "tasks",
            Self::Spec => "spec",
        }
    }

    /// The DAG node id in `schema::ARTIFACTS` for this CLI type. The two
    /// names differ for specs on purpose: the oracle's CLI subcommand takes
    /// singular `spec` while its status DAG id is plural `specs` — both
    /// independently probed, so neither side can be renamed to match the
    /// other.
    fn schema_id(self) -> &'static str {
        match self {
            Self::Spec => "specs",
            other => other.as_str(),
        }
    }

    /// Single source of truth for per-type template/output-path data:
    /// resolve through `schema::ARTIFACTS` instead of re-hardcoding the
    /// constants here, so a schema-side change cannot silently desync
    /// `new artifact` from `status` (`artifact_types_map_onto_schema_artifacts`
    /// pins the mapping).
    fn definition(self) -> &'static crate::schema::ArtifactDefinition {
        crate::schema::artifacts()
            .iter()
            .find(|artifact| artifact.id == self.schema_id())
            .expect("every ArtifactType has a matching schema::ARTIFACTS entry")
    }

    fn template(self) -> &'static str {
        self.definition().template
    }

    fn path(self, change_dir: &std::path::Path, capability: Option<&str>) -> PathBuf {
        match self {
            Self::Spec => change_dir
                .join("specs")
                .join(capability.expect("spec capability was validated"))
                .join("spec.md"),
            other => change_dir.join(other.definition().output_path),
        }
    }
}

fn is_valid_capability(capability: &str) -> bool {
    !capability.is_empty()
        && !capability.starts_with('-')
        && !capability.ends_with('-')
        && capability
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate_content(artifact_type: ArtifactType, content: &str) -> Result<()> {
    match artifact_type {
        ArtifactType::Proposal => {
            let lowercase = content.to_lowercase();
            if ["## why", "## problem", "## summary"]
                .iter()
                .any(|heading| lowercase.contains(heading))
            {
                Ok(())
            } else {
                Err(anyhow!(
                    "Proposal must contain a ## Why, ## Problem, or ## Summary section"
                ))
            }
        }
        ArtifactType::Design => {
            if content.to_lowercase().contains("## context") {
                Ok(())
            } else {
                Err(anyhow!("Design must contain a ## Context section"))
            }
        }
        ArtifactType::Tasks => {
            if ["- [ ]", "* [ ]", "+ [ ]"]
                .iter()
                .any(|checkbox| content.contains(checkbox))
            {
                Ok(())
            } else {
                Err(anyhow!("Tasks must contain at least one checkbox (- [ ])"))
            }
        }
        ArtifactType::Spec => {
            const HEADINGS: [&str; 4] = [
                "## ADDED Requirements",
                "## MODIFIED Requirements",
                "## REMOVED Requirements",
                "## RENAMED Requirements",
            ];
            if content.lines().any(|line| HEADINGS.contains(&line.trim())) {
                Ok(())
            } else {
                Err(anyhow!(
                    "Delta spec parse error: Invalid format: Delta spec must contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)"
                ))
            }
        }
    }
}

pub fn create(
    cfg: &crate::Config,
    type_name: &str,
    capability: Option<&str>,
    explicit_change: Option<&str>,
    stdin_content: Option<&str>,
    force: bool,
) -> Result<NewArtifactOutcome> {
    // No schema gate here, deliberately: probed on v2.3.1, `new artifact`
    // resolves no schema at all. With the change's `.openspec.yaml` recording
    // an unresolvable `schema:`, the oracle still exits 0 and writes the
    // artifact from its built-in template, and `new artifact` has no `--schema`
    // flag. An earlier revision of #117 added a gate here; that both diverged
    // from the oracle and reordered this function's oracle-probed check
    // sequence (`docs/reverse-engineering/artifact-workflow.md`), which PR #48
    // review had already ruled must not happen without a probe.
    let change_name = crate::change::resolve(cfg, explicit_change)?;
    let artifact_type = ArtifactType::parse(type_name)?;
    let change = crate::change::try_load(cfg, &change_name)?.ok_or_else(|| {
        // Probed against Spectra.app 2.3.1: unlike status, this error has no period.
        anyhow!("Change '{change_name}' not found")
    })?;

    if artifact_type == ArtifactType::Spec {
        let capability = capability.ok_or_else(|| {
            anyhow!(
                "Capability name is required for spec type. Usage: spectra new artifact spec <capability> --change <name>"
            )
        })?;
        if !is_valid_capability(capability) {
            return Err(anyhow!(
                "Invalid capability name '{capability}'. Must be kebab-case (e.g., user-auth, data-export)"
            ));
        }
    }

    let path = artifact_type.path(&change.dir, capability);
    if path.exists() && !force {
        return Err(anyhow!(
            "Artifact already exists: {}. Use --force to overwrite",
            path.display()
        ));
    }

    if let Some(content) = stdin_content {
        if content.trim().is_empty() {
            return Err(anyhow!("No content received from stdin"));
        }
        validate_content(artifact_type, content)?;
    }

    let content = stdin_content.unwrap_or_else(|| artifact_type.template());
    let parent = path.parent().expect("artifact paths always have a parent");
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating artifact directory {}", parent.display()))?;

    // The target path only ever appears as a fully-written file: content goes
    // to a uniquely-named sibling temp file first (`create_new`, so a
    // concurrent in-process caller can't share it and a pre-planted symlink
    // can't redirect the write), then is installed atomically — `rename`
    // (replace) for --force, `hard_link` (no-clobber) otherwise. This closes
    // the TOCTOU window after the `path.exists()` check above (kept there
    // because the probed error ORDER places already-exists before the
    // stdin/content checks): a concurrent creator that wins the race gets the
    // oracle-aligned error, and a failed write can never leave a partial
    // artifact at the target for `status` to misread as done.
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let file_name = path.file_name().expect("artifact paths end in a file name");
    let tmp = parent.join(format!(
        ".{}.tmp-{}-{}",
        file_name.to_string_lossy(),
        std::process::id(),
        TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    let write_result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut file| {
            use std::io::Write as _;
            file.write_all(content.as_bytes())
        });
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing artifact {}", path.display()));
    }
    let install_result = if force {
        // Success consumes the temp file.
        std::fs::rename(&tmp, &path)
    } else {
        // Atomic no-clobber install; the temp file remains and is removed
        // below in both outcomes.
        std::fs::hard_link(&tmp, &path)
    };
    if !force || install_result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    match install_result {
        Ok(()) => {}
        Err(e) if !force && e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(anyhow!(
                "Artifact already exists: {}. Use --force to overwrite",
                path.display()
            ));
        }
        Err(e) => {
            return Err(e).with_context(|| format!("writing artifact {}", path.display()));
        }
    }

    Ok(NewArtifactOutcome {
        artifact: artifact_type.as_str(),
        change: change_name,
        path,
        validated: stdin_content.is_some(),
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn config(root: &std::path::Path) -> crate::Config {
        crate::Config {
            root: root.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    fn add_change(cfg: &crate::Config, name: &str) -> std::path::PathBuf {
        let dir = cfg.changes_dir().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tmp_residue(change_dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(change_dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect()
    }

    #[test]
    fn concurrent_non_force_creates_yield_exactly_one_winner() {
        // Pins the atomic no-clobber install (hard_link): when N callers race
        // past the early exists-check, exactly one wins, the rest get the
        // oracle-aligned already-exists error, the winner's content is intact
        // (never interleaved), and no temp files are left behind.
        let tmp = TempDir::new("race-no-force");
        let cfg = config(&tmp);
        let change_dir = add_change(&cfg, "demo-feature");

        let root = tmp.to_path_buf();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8u32)
            .map(|i| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let cfg = config(&root);
                    barrier.wait();
                    create(
                        &cfg,
                        "proposal",
                        None,
                        Some("demo-feature"),
                        Some(&format!("## Why racer {i}")),
                        false,
                    )
                    .map(|_| i)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        let winners: Vec<u32> = results
            .iter()
            .filter_map(|r| r.as_ref().ok().copied())
            .collect();
        assert_eq!(winners.len(), 1, "exactly one racer must win: {results:?}");
        for err in results.iter().filter_map(|r| r.as_ref().err()) {
            assert!(err.to_string().contains("Artifact already exists"));
        }
        assert_eq!(
            std::fs::read_to_string(change_dir.join("proposal.md")).unwrap(),
            format!("## Why racer {}", winners[0])
        );
        assert_eq!(tmp_residue(&change_dir), Vec::<String>::new());
    }

    #[test]
    fn concurrent_force_creates_do_not_interleave_and_leave_no_temp_files() {
        // Pins the unique-temp + atomic-rename replace: concurrent in-process
        // --force writers (same PID — the temp name must not collide) all
        // succeed, the surviving content is exactly one writer's payload, and
        // no temp files are left behind.
        let tmp = TempDir::new("race-force");
        let cfg = config(&tmp);
        let change_dir = add_change(&cfg, "demo-feature");

        let root = tmp.to_path_buf();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8u32)
            .map(|i| {
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let cfg = config(&root);
                    barrier.wait();
                    create(
                        &cfg,
                        "proposal",
                        None,
                        Some("demo-feature"),
                        Some(&format!("## Why force-racer {i}")),
                        true,
                    )
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let final_content = std::fs::read_to_string(change_dir.join("proposal.md")).unwrap();
        assert!(
            (0..8u32).any(|i| final_content == format!("## Why force-racer {i}")),
            "final content must be exactly one racer's payload, got: {final_content:?}"
        );
        assert_eq!(tmp_residue(&change_dir), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn force_write_through_a_symlinked_artifact_path_cannot_escape_the_change_dir() {
        // Security: a change dir is untrusted input. If `proposal.md` is a
        // symlink pointing outside the tree, `--force` must not overwrite the
        // link target. The oracle follows the link (a shared vulnerability);
        // the temp-file + atomic-`rename` install replaces the symlink entry
        // itself instead, so the target is untouched. Pinned here because a
        // future change to the write path (e.g. back to a direct
        // `fs::write(path, ...)`) would silently reintroduce the escape.
        let tmp = TempDir::new("symlink-escape");
        let cfg = config(&tmp);
        let change_dir = add_change(&cfg, "demo-feature");

        let outside = tmp.join("outside-secret.txt");
        std::fs::write(&outside, "PRECIOUS").unwrap();
        std::os::unix::fs::symlink(&outside, change_dir.join("proposal.md")).unwrap();

        create(
            &cfg,
            "proposal",
            None,
            Some("demo-feature"),
            Some("## Why replacement"),
            true,
        )
        .unwrap();

        // The link target is untouched, and proposal.md is now a regular file
        // holding the new content (the symlink entry was replaced, not followed).
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "PRECIOUS");
        let meta = std::fs::symlink_metadata(change_dir.join("proposal.md")).unwrap();
        assert!(meta.file_type().is_file());
        assert_eq!(
            std::fs::read_to_string(change_dir.join("proposal.md")).unwrap(),
            "## Why replacement"
        );
    }

    #[test]
    fn artifact_types_map_onto_schema_artifacts() {
        // Locks the CLI-type <-> schema-DAG mapping both ways: every
        // ArtifactType resolves to a schema::ARTIFACTS entry (definition()
        // would panic otherwise), and no schema artifact lacks a CLI type --
        // so adding/renaming on one side without the other fails here instead
        // of desyncing `new artifact` from `status` at runtime.
        let types = [
            ArtifactType::Proposal,
            ArtifactType::Design,
            ArtifactType::Tasks,
            ArtifactType::Spec,
        ];
        for artifact_type in types {
            assert_eq!(artifact_type.definition().id, artifact_type.schema_id());
        }
        let mut mapped: Vec<&str> = types.iter().map(|t| t.schema_id()).collect();
        mapped.sort_unstable();
        let mut schema_ids: Vec<&str> = crate::schema::artifacts().iter().map(|a| a.id).collect();
        schema_ids.sort_unstable();
        assert_eq!(mapped, schema_ids);
    }

    #[test]
    fn parses_all_supported_artifact_types() {
        assert_eq!(
            ArtifactType::parse("proposal").unwrap(),
            ArtifactType::Proposal
        );
        assert_eq!(ArtifactType::parse("design").unwrap(), ArtifactType::Design);
        assert_eq!(ArtifactType::parse("tasks").unwrap(), ArtifactType::Tasks);
        assert_eq!(ArtifactType::parse("spec").unwrap(), ArtifactType::Spec);
    }

    #[test]
    fn capability_names_follow_the_probed_kebab_rule() {
        for valid in ["user-auth", "a--b", "cap2-v3", "a", "1"] {
            assert!(is_valid_capability(valid), "expected {valid:?} to be valid");
        }
        for invalid in ["", "Bad_Name", "bad-", "-bad", "has space"] {
            assert!(
                !is_valid_capability(invalid),
                "expected {invalid:?} to be invalid"
            );
        }
    }

    #[test]
    fn validates_stdin_content_with_each_types_probed_rule() {
        for content in ["prefix ## WHY suffix", "## Problem", "x ## summary y"] {
            validate_content(ArtifactType::Proposal, content).unwrap();
        }
        assert_eq!(
            validate_content(ArtifactType::Proposal, "## Motivation")
                .unwrap_err()
                .to_string(),
            "Proposal must contain a ## Why, ## Problem, or ## Summary section"
        );

        validate_content(ArtifactType::Design, "prefix ## CONTEXT suffix").unwrap();
        assert_eq!(
            validate_content(ArtifactType::Design, "# Context")
                .unwrap_err()
                .to_string(),
            "Design must contain a ## Context section"
        );

        for content in ["- [ ] one", "* [ ] two", "+ [ ] three"] {
            validate_content(ArtifactType::Tasks, content).unwrap();
        }
        assert_eq!(
            validate_content(ArtifactType::Tasks, "- [x] done")
                .unwrap_err()
                .to_string(),
            "Tasks must contain at least one checkbox (- [ ])"
        );

        for content in [
            "## ADDED Requirements",
            "before\n  ## MODIFIED Requirements\nafter",
            "## REMOVED Requirements",
            "## RENAMED Requirements",
        ] {
            validate_content(ArtifactType::Spec, content).unwrap();
        }
        for content in [
            "## added Requirements",
            "## ADDED Requirements extra",
            "prefix ## ADDED Requirements",
        ] {
            assert_eq!(
                validate_content(ArtifactType::Spec, content)
                    .unwrap_err()
                    .to_string(),
                "Delta spec parse error: Invalid format: Delta spec must contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)"
            );
        }
    }

    #[test]
    fn create_runs_probed_checks_in_order_and_does_not_write_on_validation_failure() {
        let root = TempDir::new("check-order");
        let cfg = config(&root);

        assert_eq!(
            create(&cfg, "bogus", None, None, None, false)
                .unwrap_err()
                .to_string(),
            "No active changes. Create one with: spectra new change <name>"
        );
        assert_eq!(
            create(&cfg, "bogus", None, Some("ghost"), None, false)
                .unwrap_err()
                .to_string(),
            "Unknown artifact type 'bogus'. Valid types: proposal, design, tasks, spec"
        );
        assert_eq!(
            create(&cfg, "spec", None, Some("ghost"), None, false)
                .unwrap_err()
                .to_string(),
            "Change 'ghost' not found"
        );

        let change_dir = add_change(&cfg, "demo");
        assert_eq!(
            create(&cfg, "spec", None, Some("demo"), None, false)
                .unwrap_err()
                .to_string(),
            "Capability name is required for spec type. Usage: spectra new artifact spec <capability> --change <name>"
        );
        std::fs::create_dir_all(change_dir.join("specs").join("bad-")).unwrap();
        std::fs::write(
            change_dir.join("specs").join("bad-").join("spec.md"),
            "existing",
        )
        .unwrap();
        assert_eq!(
            create(&cfg, "spec", Some("bad-"), Some("demo"), Some(""), false,)
                .unwrap_err()
                .to_string(),
            "Invalid capability name 'bad-'. Must be kebab-case (e.g., user-auth, data-export)"
        );

        let proposal = change_dir.join("proposal.md");
        std::fs::write(&proposal, "existing").unwrap();
        assert_eq!(
            create(&cfg, "proposal", None, Some("demo"), Some("  \n"), false,)
                .unwrap_err()
                .to_string(),
            format!(
                "Artifact already exists: {}. Use --force to overwrite",
                proposal.display()
            )
        );
        std::fs::remove_file(&proposal).unwrap();
        assert_eq!(
            create(&cfg, "proposal", None, Some("demo"), Some("  \n"), false,)
                .unwrap_err()
                .to_string(),
            "No content received from stdin"
        );
        assert_eq!(
            create(
                &cfg,
                "spec",
                Some("new-cap"),
                Some("demo"),
                Some("## lowercase requirements"),
                false,
            )
            .unwrap_err()
            .to_string(),
            "Delta spec parse error: Invalid format: Delta spec must contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)"
        );
        assert!(!change_dir.join("specs").join("new-cap").exists());
    }
}
