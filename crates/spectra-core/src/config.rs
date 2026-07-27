//! Project-level configuration from `.spectra.yaml`.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The directory (relative to project root) that holds `changes/` and `specs/`.
/// `spectra init` defaults this to `openspec`.
pub const DEFAULT_SPEC_DIR: &str = "openspec";

#[derive(Debug, Clone, Deserialize, Default)]
struct RawConfig {
    spec_dir: Option<String>,
    locale: Option<String>,
    claude_slash_commands: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Absolute project root (the directory containing `.spectra.yaml`).
    pub root: PathBuf,
    /// Resolved spec directory name (e.g. `openspec` or `docs/specs`).
    pub spec_dir: String,
    /// Locale for AI-generated artifacts (display only here).
    pub locale: Option<String>,
    /// Whether Claude's optional `/spectra:X` command files are generated.
    pub claude_slash_commands: bool,
}

impl Config {
    /// Load `.spectra.yaml` from `root`. A missing file yields defaults, but an
    /// absent file means the project was never `spectra init`-ed; callers that
    /// require initialization should check [`Config::is_initialized`].
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".spectra.yaml");
        let raw: RawConfig = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            serde_yaml::from_str(&text).with_context(|| {
                format!(
                    "parsing {} (if this file is corrupted, delete it and re-run 'spectra init' \
                     -- this resets spec_dir/locale/claude_slash_commands to their defaults)",
                    path.display()
                )
            })?
        } else {
            RawConfig::default()
        };
        Ok(Config {
            root: root.to_path_buf(),
            spec_dir: raw.spec_dir.unwrap_or_else(|| DEFAULT_SPEC_DIR.to_string()),
            locale: raw.locale,
            claude_slash_commands: raw.claude_slash_commands.unwrap_or(false),
        })
    }

    /// A project is initialized once `.spectra.yaml` exists.
    pub fn is_initialized(root: &Path) -> bool {
        root.join(".spectra.yaml").exists()
    }

    /// Absolute path to `<spec_dir>/changes`.
    pub fn changes_dir(&self) -> PathBuf {
        self.root.join(&self.spec_dir).join("changes")
    }

    /// Absolute path to `<spec_dir>/specs`.
    pub fn specs_dir(&self) -> PathBuf {
        self.root.join(&self.spec_dir).join("specs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn claude_slash_commands_defaults_false_and_only_true_enables_it() {
        let absent = TempDir::new("config-slash-absent");
        assert!(!Config::load(&absent).unwrap().claude_slash_commands);

        let explicit_false = TempDir::new("config-slash-false");
        std::fs::write(
            explicit_false.join(".spectra.yaml"),
            "claude_slash_commands: false\n",
        )
        .unwrap();
        assert!(!Config::load(&explicit_false).unwrap().claude_slash_commands);

        let enabled = TempDir::new("config-slash-true");
        std::fs::write(
            enabled.join(".spectra.yaml"),
            "claude_slash_commands: true\n",
        )
        .unwrap();
        assert!(Config::load(&enabled).unwrap().claude_slash_commands);
    }
}
