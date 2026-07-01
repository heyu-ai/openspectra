//! Capability spec discovery.
//!
//! On disk a spec is a directory `<spec_dir>/specs/<capability>/spec.md`,
//! distinct from `changes/<name>/` (a proposed delta against these specs).

use std::path::PathBuf;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct Spec {
    pub name: String,
    pub dir: PathBuf,
}

impl Spec {
    pub fn spec_md(&self) -> PathBuf {
        self.dir.join("spec.md")
    }
}

/// List capability spec names (directories under `specs_dir()` containing
/// `spec.md`), sorted.
pub fn list(cfg: &Config) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(cfg.specs_dir()) else {
        return names;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.join("spec.md").is_file() {
            continue;
        }
        names.push(entry.file_name().to_string_lossy().to_string());
    }
    names.sort();
    names
}

/// Load a single spec by capability name. Errors if `spec.md` is missing.
pub fn load(cfg: &Config, name: &str) -> anyhow::Result<Spec> {
    let dir = cfg.specs_dir().join(name);
    if !dir.join("spec.md").is_file() {
        return Err(anyhow::anyhow!(
            "spec '{name}' not found in {}",
            cfg.specs_dir().display()
        ));
    }
    Ok(Spec { name: name.to_string(), dir })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &std::path::Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn lists_capability_dirs_containing_spec_md() {
        let tmp = tempfile_dir();
        let cfg = Config { root: tmp.clone(), spec_dir: "openspec".to_string(), locale: None };
        write(&cfg.specs_dir().join("auth").join("spec.md"), "# Auth\n");
        write(&cfg.specs_dir().join("billing").join("spec.md"), "# Billing\n");
        // A directory without spec.md must not count as a spec.
        fs::create_dir_all(cfg.specs_dir().join("empty-dir")).unwrap();

        let names = list(&cfg);
        assert_eq!(names, vec!["auth".to_string(), "billing".to_string()]);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn load_errors_when_spec_md_missing() {
        let tmp = tempfile_dir();
        let cfg = Config { root: tmp.clone(), spec_dir: "openspec".to_string(), locale: None };
        fs::create_dir_all(cfg.specs_dir().join("ghost")).unwrap();

        assert!(load(&cfg, "ghost").is_err());
        assert!(load(&cfg, "does-not-exist").is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "spectra-spec-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
