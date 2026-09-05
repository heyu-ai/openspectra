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
use crate::names::is_valid_name;

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

fn is_valid_capability_id(id: &str) -> bool {
    !id.is_empty() && !id.contains('\\') && id.split('/').all(is_valid_name)
}

/// List capability spec names (directories under `specs_dir()` containing
/// `spec.md`), sorted. A missing `specs_dir()` is not an error (mirrors
/// `change::list_active`); any other I/O failure (e.g. permissions) is.
pub fn list(cfg: &Config) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut visited = std::collections::HashSet::new();
    collect_specs(
        &cfg.specs_dir(),
        std::path::Path::new(""),
        &mut visited,
        &mut names,
    )?;
    names.sort();
    Ok(names)
}

fn collect_specs(
    dir: &std::path::Path,
    relative: &std::path::Path,
    visited: &mut std::collections::HashSet<PathBuf>,
    names: &mut Vec<String>,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
    };
    let canonical =
        std::fs::canonicalize(dir).with_context(|| format!("resolving {}", dir.display()))?;
    if !visited.insert(canonical) {
        return Ok(());
    }

    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            eprintln!(
                "warning: skipping non-UTF-8 spec entry {:?} in {}",
                entry.file_name().to_string_lossy(),
                dir.display()
            );
            continue;
        };
        let path = entry.path();
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        };
        if metadata.is_dir() {
            collect_specs(&path, &relative.join(&name), visited, names)?;
        } else if metadata.is_file() && name == "spec.md" && !relative.as_os_str().is_empty() {
            let id = relative
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            names.push(id);
        }
    }
    Ok(())
}

/// Load a spec by name if it exists. Returns `Ok(None)` only when `spec.md`
/// is genuinely absent (or `name` is invalid); any other I/O failure
/// checking for it propagates as `Err`, so callers juggling multiple
/// namespaces (e.g. `show`, which also tries `change::try_load`) can't
/// misread a permission error as "not a spec" the way a boolean
/// `Path::is_file()` check would.
pub fn try_load(cfg: &Config, name: &str) -> Result<Option<Spec>> {
    if !is_valid_capability_id(name) {
        return Ok(None);
    }
    let spec_md = cfg.specs_dir().join(name).join("spec.md");
    match std::fs::metadata(&spec_md) {
        Ok(m) if m.is_file() => {}
        Ok(_) => return Ok(None),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", spec_md.display())),
    }
    load(cfg, name).map(Some)
}

/// Load a single spec by capability name. Errors if `spec.md` is missing (or
/// unreadable for any other reason). `name` must be a single path component
/// (no separators or `..`) — `spectra show <spec>` (spectra-cli) passes the
/// raw CLI argument straight through, so this traversal guard is
/// load-bearing, not defensive-for-a-hypothetical-future-caller.
pub fn load(cfg: &Config, name: &str) -> Result<Spec> {
    if !is_valid_capability_id(name) {
        return Err(anyhow::anyhow!("invalid spec name '{name}'"));
    }
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
        let tmp = TempDir::new("spec");
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
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
    }

    #[test]
    fn list_is_empty_when_specs_dir_missing_entirely() {
        let tmp = TempDir::new("spec");
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        assert_eq!(list(&cfg).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn list_is_empty_when_specs_dir_present_but_has_no_spec_md() {
        let tmp = TempDir::new("spec");
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        fs::create_dir_all(cfg.specs_dir().join("empty-dir")).unwrap();

        assert_eq!(list(&cfg).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn load_errors_when_spec_md_missing() {
        let tmp = TempDir::new("spec");
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        fs::create_dir_all(cfg.specs_dir().join("ghost")).unwrap();

        assert!(load(&cfg, "ghost").is_err());
        assert!(load(&cfg, "does-not-exist").is_err());
    }

    #[test]
    fn load_rejects_path_traversal_names() {
        let tmp = TempDir::new("spec");
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        // A spec.md outside specs_dir() that a traversal attempt could reach.
        write(&tmp.join("secret.md"), "outside\n");

        assert!(load(&cfg, "../secret").is_err());
        assert!(load(&cfg, "..").is_err());
        assert!(load(&cfg, "sub/dir").is_err());
        assert!(load(&cfg, "").is_err());
    }

    #[test]
    fn load_succeeds_for_valid_spec() {
        let tmp = TempDir::new("spec");
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        write(&cfg.specs_dir().join("auth").join("spec.md"), "# Auth\n");

        let sp = load(&cfg, "auth").unwrap();
        assert_eq!(sp.name, "auth");
        assert_eq!(sp.spec_md(), cfg.specs_dir().join("auth").join("spec.md"));
    }

    #[cfg(unix)]
    #[test]
    fn list_follows_symlinked_spec_directory() {
        let tmp = TempDir::new("spec");
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        let real_dir = tmp.join("real-auth");
        write(&real_dir.join("spec.md"), "# Auth\n");
        std::fs::create_dir_all(cfg.specs_dir()).unwrap();
        std::os::unix::fs::symlink(&real_dir, cfg.specs_dir().join("auth")).unwrap();

        // A capability dir that's a symlink to a real directory must still be
        // found (DirEntry::file_type() does not follow symlinks; fs::metadata does).
        assert_eq!(list(&cfg).unwrap(), vec!["auth".to_string()]);
    }

    // macOS (APFS/HFS+) rejects non-UTF-8 filenames at the syscall level, so
    // this is only constructible on Linux (ext4 et al. allow arbitrary bytes).
    #[cfg(target_os = "linux")]
    #[test]
    fn list_skips_non_utf8_directory_names() {
        use std::os::unix::ffi::OsStrExt;

        let tmp = TempDir::new("spec");
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        write(&cfg.specs_dir().join("auth").join("spec.md"), "# Auth\n");
        let bad_name = std::ffi::OsStr::from_bytes(b"bad-\xFF-name");
        write(&cfg.specs_dir().join(bad_name).join("spec.md"), "# Bad\n");

        // The non-UTF-8 entry is skipped (with a stderr warning), not returned
        // and not a hard failure for the whole listing.
        assert_eq!(list(&cfg).unwrap(), vec!["auth".to_string()]);
    }

    /// RAII guard for a per-test scratch directory: removes it on drop even
    /// when the test panics partway through (an assertion failure must not
    /// leak the directory).
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            // Nanosecond timestamps alone can collide between threads running
            // concurrently (observed in practice under `cargo test`'s default
            // parallel harness); an atomic counter guarantees uniqueness.
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-{label}-test-{}-{}-{seq}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl std::ops::Deref for TempDir {
        type Target = std::path::Path;
        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
