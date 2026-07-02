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
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Absolute project root (the directory containing `.spectra.yaml`).
    pub root: PathBuf,
    /// Resolved spec directory name (e.g. `openspec` or `docs/specs`).
    pub spec_dir: String,
    /// Locale for AI-generated artifacts (display only here).
    pub locale: Option<String>,
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
            serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        } else {
            RawConfig::default()
        };
        Ok(Config {
            root: root.to_path_buf(),
            spec_dir: raw.spec_dir.unwrap_or_else(|| DEFAULT_SPEC_DIR.to_string()),
            locale: raw.locale,
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
