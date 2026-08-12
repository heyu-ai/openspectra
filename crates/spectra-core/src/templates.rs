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
pub fn list(cfg: &Config, schema_name: Option<&str>) -> Result<Vec<TemplateListing>> {
    schema::require_supported(cfg, schema_name, None)?;
    Ok(schema::SCHEMA_ARTIFACT_ORDER
        .iter()
        .map(|artifact_id| {
            let artifact = schema::artifacts()
                .iter()
                .find(|artifact| artifact.id == *artifact_id)
                .expect("schema artifact order must reference a definition");
            TemplateListing {
                artifact_id: artifact.id,
                has_content: !artifact.template.is_empty(),
                template_name: match artifact.id {
                    "specs" => "spec.md",
                    _ => artifact.output_path,
                },
            }
        })
        .collect())
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
}
