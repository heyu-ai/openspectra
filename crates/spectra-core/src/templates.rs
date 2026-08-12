//! Workflow template metadata exposed by the `templates` command.

use anyhow::Result;
use serde::Serialize;

use crate::{schema, Config};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateListing {
    pub artifact_id: &'static str,
    pub has_content: bool,
    pub template_name: &'static str,
}

/// Return template metadata in the schema's authoring order.
///
/// Only an explicit `--schema` name is validated against the built-in
/// registry. With no explicit name, this always reports the built-in
/// schema's templates regardless of a project's *configured* schema —
/// matching the command's "available outside an initialized project"
/// contract, since it never loads a custom schema's templates either way.
pub fn list(cfg: &Config, schema_name: Option<&str>) -> Result<Vec<TemplateListing>> {
    if let Some(name) = schema_name {
        schema::require_supported(cfg, Some(name), None)?;
    }
    Ok(schema::SCHEMA_ARTIFACT_ORDER
        .iter()
        .map(|artifact_id| {
            // Guarded by schema.rs's own permutation test tying
            // SCHEMA_ARTIFACT_ORDER to ARTIFACTS -- a desync there fails CI
            // before it can reach this expect() at runtime.
            let artifact = schema::artifacts()
                .iter()
                .find(|artifact| artifact.id == *artifact_id)
                .expect("schema artifact order must reference a definition");
            TemplateListing {
                artifact_id: artifact.id,
                has_content: has_content(artifact.template),
                template_name: match artifact.id {
                    "specs" => "spec.md",
                    _ => artifact.output_path,
                },
            }
        })
        .collect())
}

fn has_content(template: &str) -> bool {
    !template.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn config(root: &std::path::Path) -> Config {
        Config {
            root: root.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    #[test]
    fn built_in_templates_match_oracle_order_names_and_content_flags() {
        let root = TempDir::new("templates-built-in");
        let templates = list(&config(&root), None).unwrap();
        assert_eq!(
            templates,
            vec![
                TemplateListing {
                    artifact_id: "proposal",
                    has_content: true,
                    template_name: "proposal.md",
                },
                TemplateListing {
                    artifact_id: "specs",
                    has_content: true,
                    template_name: "spec.md",
                },
                TemplateListing {
                    artifact_id: "design",
                    has_content: true,
                    template_name: "design.md",
                },
                TemplateListing {
                    artifact_id: "tasks",
                    has_content: true,
                    template_name: "tasks.md",
                },
            ]
        );
    }

    #[test]
    fn unknown_schema_uses_the_probed_error() {
        let root = TempDir::new("templates-unknown");
        assert_eq!(
            list(&config(&root), Some("bogus")).unwrap_err().to_string(),
            "Schema not found: Schema 'bogus' not found in project, user, or built-in locations"
        );
    }

    #[test]
    fn has_content_is_false_only_for_an_empty_template() {
        assert!(!has_content(""));
        assert!(has_content("x"));
    }

    #[test]
    fn template_name_falls_back_to_output_path_only_when_it_is_not_a_glob() {
        // `template_name` piggybacks on `output_path` for every artifact
        // except "specs" (whose `output_path` is the glob
        // "specs/**/*.md", hence the hardcoded "spec.md" override). This
        // guards that coupling: a future artifact whose `output_path`
        // becomes glob-shaped under a different id would otherwise leak
        // the raw glob string as `templateName` with nothing failing.
        let root = TempDir::new("templates-output-path-guard");
        let templates = list(&config(&root), None).unwrap();
        for template in &templates {
            assert!(
                !template.template_name.contains('*'),
                "artifact {:?}'s template_name leaked a glob: {:?}",
                template.artifact_id,
                template.template_name
            );
        }
    }

    #[test]
    fn a_configured_but_unsupported_schema_does_not_block_the_bare_command() {
        // With no explicit `--schema`, `templates` always reports the
        // built-in schema, regardless of what a project's config.yaml
        // happens to name -- it never loads that schema's templates
        // either way, so failing loud here would only break the command's
        // own "available outside an initialized project" contract.
        let root = TempDir::new("templates-configured-unsupported");
        std::fs::create_dir_all(root.join("openspec")).unwrap();
        std::fs::write(
            root.join("openspec").join("config.yaml"),
            "schema: mycustom\n",
        )
        .unwrap();

        let templates = list(&config(&root), None).unwrap();
        assert_eq!(templates.len(), 4);
        assert_eq!(templates[0].artifact_id, "proposal");
    }
}
