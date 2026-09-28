//! `spectra show` 的資料（oracle 3.0.0，見 `docs/reverse-engineering/list-show.md`）。

use anyhow::{Context, Result};
use serde::Serialize;
use std::io::ErrorKind;
use std::path::Path;

use crate::config::Config;

/// `show <change> --json`；serde_json 的 Value 物件依字母排序 key，與 oracle 相同。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeView {
    pub created: Option<String>,
    pub delta_specs: Vec<String>,
    pub design: Option<String>,
    pub name: String,
    pub proposal: Option<String>,
    pub schema: Option<String>,
    pub tasks: Option<String>,
}

/// `show <spec> --json`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpecView {
    pub files: Vec<SpecFile>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpecFile {
    pub content: String,
    pub name: String,
}

pub enum View {
    Change(ChangeView),
    Spec(SpecView),
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// YAML 純量原樣轉成字串（`20260210` → `"20260210"`）。
fn scalar_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// `.openspec.yaml` 必須同時有 `schema` 與 `created`，否則兩者皆為 `None`（oracle 以
/// 兩欄都必填的 struct 反序列化）。檔案缺失或無法解析時不警告，與 oracle 相同。
pub(crate) fn schema_and_created(dir: &Path) -> (Option<String>, Option<String>) {
    let parsed = std::fs::read_to_string(dir.join(".openspec.yaml"))
        .ok()
        .and_then(|text| serde_yaml::from_str::<serde_yaml::Value>(&text).ok());
    let Some(value) = parsed else {
        return (None, None);
    };
    match (
        value.get("schema").and_then(scalar_string),
        value.get("created").and_then(scalar_string),
    ) {
        (Some(schema), Some(created)) => (Some(schema), Some(created)),
        _ => (None, None),
    }
}

/// `dir` 底下所有 `*.md`（遞迴），回傳以 `/` 分隔的相對路徑，依位元組排序。
fn markdown_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                if let Ok(rel) = path.strip_prefix(dir) {
                    out.push(
                        rel.components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join("/"),
                    );
                }
            }
        }
    }
    out.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    out
}

/// change 優先於同名的 spec；`specs/` 下任何目錄都可當 spec（即使沒有 `spec.md`）。
pub fn resolve(cfg: &Config, item: &str) -> Result<View> {
    if let Some(ch) = crate::change::try_load(cfg, item)? {
        let dir = cfg.root.join(&ch.dir);
        let (schema, created) = schema_and_created(&dir);
        return Ok(View::Change(ChangeView {
            created,
            delta_specs: markdown_files(&dir.join("specs")),
            design: read_optional(&dir.join("design.md"))?,
            name: item.to_string(),
            proposal: read_optional(&dir.join("proposal.md"))?,
            schema,
            tasks: read_optional(&dir.join("tasks.md"))?,
        }));
    }
    let spec_dir = cfg.specs_dir().join(item);
    if !item.is_empty() && spec_dir.is_dir() {
        let mut files = Vec::new();
        for name in markdown_files(&spec_dir) {
            let content = std::fs::read_to_string(spec_dir.join(&name))
                .with_context(|| format!("reading {}", spec_dir.join(&name).display()))?;
            files.push(SpecFile { content, name });
        }
        return Ok(View::Spec(SpecView {
            files,
            name: item.to_string(),
        }));
    }
    anyhow::bail!("Item '{item}' not found as a change or spec.")
}

/// 人類輸出（oracle 3.0.0）。
pub fn render_human(view: &View) -> String {
    let mut out = String::new();
    match view {
        View::Change(c) => {
            out.push_str(&format!("Change: {}\n", c.name));
            if let (Some(schema), Some(created)) = (&c.schema, &c.created) {
                out.push_str(&format!("Schema: {schema}\nCreated: {created}\n"));
            }
            if let Some(proposal) = &c.proposal {
                out.push_str(&format!("\n--- Proposal ---\n{proposal}\n"));
            }
            if !c.delta_specs.is_empty() {
                out.push_str("\n--- Delta Specs ---\n");
                for path in &c.delta_specs {
                    out.push_str(&format!("  {path}\n"));
                }
            }
        }
        View::Spec(s) => {
            out.push_str(&format!("Spec: {}\n", s.name));
            for file in &s.files {
                out.push_str(&format!("\n--- {} ---\n{}\n", file.name, file.content));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(label: &str) -> (std::path::PathBuf, Config) {
        let root =
            std::env::temp_dir().join(format!("spectra-show-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("openspec/changes/archive")).unwrap();
        std::fs::create_dir_all(root.join("openspec/specs")).unwrap();
        let cfg = Config {
            root: root.clone(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        (root, cfg)
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn change_view_matches_the_oracle_shape() {
        let (root, cfg) = project("change");
        let dir = root.join("openspec/changes/add-thing");
        write(
            &dir.join(".openspec.yaml"),
            "schema: spec-driven\ncreated: 20260210\nfoo: bar\n",
        );
        write(&dir.join("proposal.md"), "## Why\n\nx.\n");
        write(&dir.join("design.md"), "");
        write(&dir.join("specs/thing/spec.md"), "d\n");
        write(&dir.join("specs/Upper/spec.md"), "d\n");
        write(&dir.join("specs/top.md"), "d\n");
        write(&dir.join("specs/thing/notes.txt"), "x\n");
        let View::Change(view) = resolve(&cfg, "add-thing").unwrap() else {
            panic!("expected a change");
        };
        assert_eq!(view.created.as_deref(), Some("20260210"));
        assert_eq!(view.schema.as_deref(), Some("spec-driven"));
        assert_eq!(view.design.as_deref(), Some(""));
        assert_eq!(view.tasks, None);
        assert_eq!(
            view.delta_specs,
            vec!["Upper/spec.md", "thing/spec.md", "top.md"]
        );
        assert_eq!(
            serde_json::to_value(&view)
                .unwrap()
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec![
                "created",
                "deltaSpecs",
                "design",
                "name",
                "proposal",
                "schema",
                "tasks"
            ]
        );
        assert_eq!(
            render_human(&View::Change(view)),
            "Change: add-thing\nSchema: spec-driven\nCreated: 20260210\n\n--- Proposal ---\n## Why\n\nx.\n\n\n--- Delta Specs ---\n  Upper/spec.md\n  thing/spec.md\n  top.md\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn metadata_needs_both_schema_and_created() {
        let (root, cfg) = project("meta");
        let dir = root.join("openspec/changes/only-created");
        write(&dir.join(".openspec.yaml"), "created: 2026-01-01\n");
        let View::Change(view) = resolve(&cfg, "only-created").unwrap() else {
            panic!("expected a change");
        };
        assert_eq!((view.schema, view.created), (None, None));
        assert_eq!(view.proposal, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn any_spec_directory_resolves_and_lists_its_markdown_files() {
        let (root, cfg) = project("spec");
        write(&root.join("openspec/specs/multi/spec.md"), "# multi\n");
        write(&root.join("openspec/specs/multi/sub/deep.md"), "sub\n");
        write(&root.join("openspec/specs/multi/A.md"), "A\n");
        write(&root.join("openspec/specs/multi/x.txt"), "x\n");
        std::fs::create_dir_all(root.join("openspec/specs/emptydir")).unwrap();
        let View::Spec(view) = resolve(&cfg, "multi").unwrap() else {
            panic!("expected a spec");
        };
        assert_eq!(
            view.files
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["A.md", "spec.md", "sub/deep.md"]
        );
        let View::Spec(empty) = resolve(&cfg, "emptydir").unwrap() else {
            panic!("expected a spec");
        };
        assert_eq!(render_human(&View::Spec(empty)), "Spec: emptydir\n");
        assert_eq!(
            resolve(&cfg, "nothing-here").err().unwrap().to_string(),
            "Item 'nothing-here' not found as a change or spec."
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
