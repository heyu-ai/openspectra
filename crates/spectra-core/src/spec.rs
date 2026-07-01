//! Capability spec discovery.
//!
//! On disk a spec is a directory `<spec_dir>/specs/<capability>/spec.md`,
//! distinct from `changes/<name>/` (a proposed delta against these specs).
//! Note the on-disk shape collision: a change also has its own
//! `<spec_dir>/changes/<name>/specs/<cap>/spec.md`, which holds that change's
//! *proposed* delta, not the canonical spec this module reads.

use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result};

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
/// `spec.md`), sorted. A missing `specs_dir()` is not an error (mirrors
/// `change::list_active`); any other I/O failure (e.g. permissions) is.
pub fn list(cfg: &Config) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let entries = match std::fs::read_dir(cfg.specs_dir()) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(names),
        Err(e) => return Err(e).with_context(|| format!("reading {}", cfg.specs_dir().display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", cfg.specs_dir().display()))?;
        let path = entry.path();
        if !path.is_dir() || !path.join("spec.md").is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            eprintln!(
                "warning: skipping non-UTF-8 spec directory name in {}",
                cfg.specs_dir().display()
            );
            continue;
        };
        names.push(name);
    }
    names.sort();
    Ok(names)
}

/// Load a single spec by capability name. Errors if `spec.md` is missing (or
/// unreadable for any other reason).
pub fn load(cfg: &Config, name: &str) -> Result<Spec> {
    let dir = cfg.specs_dir().join(name);
    match std::fs::metadata(dir.join("spec.md")) {
        Ok(m) if m.is_file() => Ok(Spec {
            name: name.to_string(),
            dir,
        }),
        Ok(_) => Err(anyhow::anyhow!(
            "spec '{name}' not found in {}: spec.md is not a file",
            cfg.specs_dir().display()
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(anyhow::anyhow!(
            "spec '{name}' not found in {}",
            cfg.specs_dir().display()
        )),
        Err(e) => Err(e).with_context(|| format!("reading {}", dir.join("spec.md").display())),
    }
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
        let cfg = Config {
            root: tmp.clone(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(&cfg.specs_dir().join("auth").join("spec.md"), "# Auth\n");
        write(
            &cfg.specs_dir().join("billing").join("spec.md"),
            "# Billing\n",
        );
        // A directory without spec.md must not count as a spec.
        fs::create_dir_all(cfg.specs_dir().join("empty-dir")).unwrap();

        let names = list(&cfg).unwrap();
        assert_eq!(names, vec!["auth".to_string(), "billing".to_string()]);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn list_is_empty_when_specs_dir_missing_entirely() {
        let tmp = tempfile_dir();
        let cfg = Config {
            root: tmp.clone(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        assert_eq!(list(&cfg).unwrap(), Vec::<String>::new());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn list_is_empty_when_specs_dir_present_but_has_no_spec_md() {
        let tmp = tempfile_dir();
        let cfg = Config {
            root: tmp.clone(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        fs::create_dir_all(cfg.specs_dir().join("empty-dir")).unwrap();

        assert_eq!(list(&cfg).unwrap(), Vec::<String>::new());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn load_errors_when_spec_md_missing() {
        let tmp = tempfile_dir();
        let cfg = Config {
            root: tmp.clone(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        fs::create_dir_all(cfg.specs_dir().join("ghost")).unwrap();

        assert!(load(&cfg, "ghost").is_err());
        assert!(load(&cfg, "does-not-exist").is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn load_succeeds_for_valid_spec() {
        let tmp = tempfile_dir();
        let cfg = Config {
            root: tmp.clone(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(&cfg.specs_dir().join("auth").join("spec.md"), "# Auth\n");

        let sp = load(&cfg, "auth").unwrap();
        assert_eq!(sp.name, "auth");
        assert_eq!(sp.spec_md(), cfg.specs_dir().join("auth").join("spec.md"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    // macOS (APFS/HFS+) rejects non-UTF-8 filenames at the syscall level, so
    // this is only constructible on Linux (ext4 et al. allow arbitrary bytes).
    #[cfg(target_os = "linux")]
    #[test]
    fn list_skips_non_utf8_directory_names() {
        use std::os::unix::ffi::OsStrExt;

        let tmp = tempfile_dir();
        let cfg = Config {
            root: tmp.clone(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(&cfg.specs_dir().join("auth").join("spec.md"), "# Auth\n");
        let bad_name = std::ffi::OsStr::from_bytes(b"bad-\xFF-name");
        write(&cfg.specs_dir().join(bad_name).join("spec.md"), "# Bad\n");

        // The non-UTF-8 entry is skipped (with a stderr warning), not returned
        // and not a hard failure for the whole listing.
        assert_eq!(list(&cfg).unwrap(), vec!["auth".to_string()]);

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
