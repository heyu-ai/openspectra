//! Design anchors: references in `design.md` to concrete code artifacts that
//! drift can verify still resolve against the current codebase.
//!
//! The four extraction regexes and the symbol stop-list are recovered verbatim
//! from the reference binary's `.rodata`.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use crate::calibration::ANCHOR_CAP;
use crate::git;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AnchorKind {
    FilePath,
    Symbol,
    Function,
    CliFlag,
}

impl AnchorKind {
    fn as_str(self) -> &'static str {
        match self {
            AnchorKind::FilePath => "FilePath",
            AnchorKind::Symbol => "Symbol",
            AnchorKind::Function => "Function",
            AnchorKind::CliFlag => "CliFlag",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Anchor {
    pub text: String,
    pub kind: AnchorKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrokenAnchor {
    pub anchor: String,
    pub category: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedAnchor {
    pub anchor: String,
    pub category: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct Resolution {
    pub broken: Vec<BrokenAnchor>,
    pub unresolved: Vec<UnresolvedAnchor>,
}

// --- recovered regexes -------------------------------------------------------
static FILE_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:src-tauri|src|crates|docs)/[\w./-]+\.(?:rs|ts|svelte|md|toml)").unwrap()
});
static CLI_FLAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"--[a-z][a-z0-9-]+").unwrap());
static SNAKE_FN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([a-z][a-z0-9]*_[a-z0-9_]+)\(").unwrap());
static CAMEL_FN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([a-z][a-z0-9]*[A-Z][a-zA-Z0-9]+)\(").unwrap());
static SYMBOL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[A-Z][a-zA-Z0-9]+\b").unwrap());

/// Common Rust types / framework words excluded from Symbol anchors so the
/// extractor does not flag ubiquitous identifiers as "drift" (recovered list).
static SYMBOL_STOPLIST: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "Context", "State", "Result", "Error", "Option", "Vec", "Box", "String", "HashMap",
        "HashSet", "PathBuf", "Ok", "Err", "Default", "Sized", "Clone", "Debug", "Display", "Fn",
        "FnMut", "FnOnce", "Future", "Pin", "Arc", "Rc", "RefCell", "Mutex", "RwLock", "Trait",
        "Module", "Struct", "Field", "Method", "Value", "Source", "Target", "Given", "Spectra",
        "CLI", "GUI", "API", "IPC",
    ]
    .into_iter()
    .collect()
});

/// Extract unique anchors from `design.md` text, deduped by string (first
/// matching category wins) and capped at [`ANCHOR_CAP`].
pub fn extract(design: &str) -> Vec<Anchor> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Anchor> = Vec::new();

    let push =
        |text: String, kind: AnchorKind, seen: &mut HashSet<String>, out: &mut Vec<Anchor>| {
            if seen.insert(text.clone()) {
                out.push(Anchor { text, kind });
            }
        };

    // Most specific categories first so a string is claimed once.
    for m in FILE_PATH_RE.find_iter(design) {
        push(
            m.as_str().to_string(),
            AnchorKind::FilePath,
            &mut seen,
            &mut out,
        );
    }
    for m in CLI_FLAG_RE.find_iter(design) {
        push(
            m.as_str().to_string(),
            AnchorKind::CliFlag,
            &mut seen,
            &mut out,
        );
    }
    for c in SNAKE_FN_RE.captures_iter(design) {
        push(c[1].to_string(), AnchorKind::Function, &mut seen, &mut out);
    }
    for c in CAMEL_FN_RE.captures_iter(design) {
        push(c[1].to_string(), AnchorKind::Function, &mut seen, &mut out);
    }
    // KNOWN DIVERGENCE: the reference binary applies an additional, undetermined
    // filter that narrows Symbol candidates to a small subset (e.g. 12 of ~83 in
    // one Chinese-prose-heavy design). The recovered regex + stop-list below is
    // exactly what the binary's `.rodata` contains, but the narrowing predicate
    // is not visible in strings and resisted black-box probing — it is the single
    // open reverse-engineering question (see docs/reverse-engineering/drift.md).
    // We extract the full regex match set, which over-counts Symbol anchors on
    // prose-dense designs and can make Structure decay read lower than the oracle.
    // FilePath / Function / CliFlag extraction matches the oracle exactly.
    for m in SYMBOL_RE.find_iter(design) {
        let s = m.as_str();
        if SYMBOL_STOPLIST.contains(s) {
            continue;
        }
        push(s.to_string(), AnchorKind::Symbol, &mut seen, &mut out);
    }

    out.truncate(ANCHOR_CAP);
    out
}

/// Resolution context: the set of tracked files plus the repo root for grep.
pub struct Resolver<'a> {
    pub root: &'a Path,
    pub tracked: &'a HashSet<String>,
    pub baseline_sha: Option<&'a str>,
}

impl Resolver<'_> {
    /// Classify non-resolving anchors as broken or unresolved.
    ///
    /// Both result lists are sorted by anchor string. A missing FilePath is
    /// broken only when it existed at the change baseline; without a usable
    /// baseline, it retains the previous broken classification.
    pub fn resolve(&self, anchors: &[Anchor]) -> Resolution {
        let needles: Vec<&str> = anchors
            .iter()
            .filter_map(|anchor| match anchor.kind {
                AnchorKind::Function | AnchorKind::Symbol => Some(anchor.text.as_str()),
                AnchorKind::FilePath | AnchorKind::CliFlag => None,
            })
            .collect();
        let resolved = git::grep_existing(self.root, &needles);

        let mut broken = Vec::new();
        let mut unresolved = Vec::new();
        for anchor in anchors {
            let category = anchor.kind.as_str().to_string();
            match anchor.kind {
                AnchorKind::FilePath => {
                    let on_disk = self.root.join(&anchor.text).exists();
                    if self.tracked.contains(&anchor.text) || on_disk {
                        continue;
                    }
                    let existed_at_baseline = self
                        .baseline_sha
                        .and_then(|sha| git::path_exists_at(self.root, sha, &anchor.text));
                    if matches!(existed_at_baseline, Some(false)) {
                        unresolved.push(UnresolvedAnchor {
                            anchor: anchor.text.clone(),
                            category,
                            reason: "forward reference".to_string(),
                        });
                    } else {
                        broken.push(BrokenAnchor {
                            anchor: anchor.text.clone(),
                            category,
                            reason: "file does not exist".to_string(),
                        });
                    }
                }
                AnchorKind::Function => {
                    if !resolved.contains(&anchor.text) {
                        unresolved.push(UnresolvedAnchor {
                            anchor: anchor.text.clone(),
                            category,
                            reason: "not first-party".to_string(),
                        });
                    }
                }
                AnchorKind::Symbol => {
                    if !resolved.contains(&anchor.text) {
                        broken.push(BrokenAnchor {
                            anchor: anchor.text.clone(),
                            category,
                            reason: "symbol not found in repo".to_string(),
                        });
                    }
                }
                AnchorKind::CliFlag => unresolved.push(UnresolvedAnchor {
                    anchor: anchor.text.clone(),
                    category,
                    reason: "no target --help".to_string(),
                }),
            }
        }
        broken.sort_by(|a, b| a.anchor.cmp(&b.anchor));
        unresolved.sort_by(|a, b| a.anchor.cmp(&b.anchor));
        Resolution { broken, unresolved }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-anchors-test-{label}-{}-{seq}-{}",
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
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn init_repo_with_file(dir: &Path, name: &str, contents: &str) {
        let run = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t.co"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.join(name), contents).unwrap();
        run(&["add", name]);
        run(&["commit", "-q", "-m", "init"]);
    }

    fn kinds(design: &str, kind: AnchorKind) -> Vec<String> {
        let mut v: Vec<String> = extract(design)
            .into_iter()
            .filter(|a| a.kind == kind)
            .map(|a| a.text)
            .collect();
        v.sort();
        v
    }

    #[test]
    fn extracts_file_paths_exactly() {
        // Oracle-verified: src/.../*.rs|ts|svelte|md|toml and docs/, crates/, src-tauri/.
        let d = "see `src/foo/bar.rs` and src-tauri/x.ts and docs/a.md and backend/x.py";
        assert_eq!(
            kinds(d, AnchorKind::FilePath),
            vec!["docs/a.md", "src-tauri/x.ts", "src/foo/bar.rs"]
        );
        // backend/x.py is NOT matched: the recovered regex is Rust/TS-stack only.
    }

    #[test]
    fn extracts_cli_flags_anywhere() {
        let d = "prose --alpha and `--beta` and a fence has --gamma-flag too";
        assert_eq!(
            kinds(d, AnchorKind::CliFlag),
            vec!["--alpha", "--beta", "--gamma-flag"]
        );
    }

    #[test]
    fn extracts_functions_snake_and_camel() {
        let d = "calls compute_effective_weight() and doThing() but not bare_word";
        assert_eq!(
            kinds(d, AnchorKind::Function),
            vec!["compute_effective_weight", "doThing"]
        );
    }

    #[test]
    fn symbol_stoplist_excludes_common_types() {
        let d = "Result and Option and MyType here";
        assert_eq!(kinds(d, AnchorKind::Symbol), vec!["MyType"]);
    }

    #[test]
    fn dedup_and_cap() {
        let d = "--flag --flag --flag";
        assert_eq!(extract(d).len(), 1);
        let many = (0..80)
            .map(|i| format!("--flag{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(extract(&many).len(), ANCHOR_CAP);
    }

    #[test]
    fn cli_flags_are_always_unresolved_without_a_target_help() {
        use std::collections::HashSet;
        let tracked: HashSet<String> = HashSet::new();
        let root = std::path::Path::new(".");
        let r = Resolver {
            root,
            tracked: &tracked,
            baseline_sha: None,
        };
        let anchors = extract("uses --some-flag");
        let resolution = r.resolve(&anchors);
        assert!(resolution.broken.is_empty());
        assert_eq!(resolution.unresolved.len(), 1);
        assert_eq!(resolution.unresolved[0].reason, "no target --help");
        assert_eq!(resolution.unresolved[0].category, "CliFlag");
    }

    #[test]
    fn missing_file_without_a_baseline_falls_back_to_broken() {
        let tracked = HashSet::new();
        let dir = TempDir::new("resolver-no-baseline");
        let resolver = Resolver {
            root: &dir,
            tracked: &tracked,
            baseline_sha: None,
        };

        let resolution = resolver.resolve(&extract("src/missing.rs"));

        assert_eq!(resolution.broken.len(), 1);
        assert_eq!(resolution.broken[0].reason, "file does not exist");
        assert!(resolution.unresolved.is_empty());
    }

    #[test]
    fn resolver_batches_large_function_and_symbol_sets() {
        let function_names: Vec<String> = (0..24).map(|i| format!("alpha_fn_{i:02}")).collect();
        let symbol_names: Vec<String> = (0..20).map(|i| format!("AlphaSym{i:02}")).collect();

        let mut design = String::new();
        for name in &function_names {
            design.push_str(name);
            design.push_str("() ");
        }
        for name in &symbol_names {
            design.push_str(name);
            design.push(' ');
        }
        let anchors = extract(&design);
        assert_eq!(anchors.len(), 44);

        let present_functions: HashSet<String> = function_names
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 3 == 0)
            .map(|(_, name)| name.clone())
            .collect();
        let present_symbols: HashSet<String> = symbol_names
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 4 == 0)
            .map(|(_, name)| name.clone())
            .collect();

        let mut repo_contents = String::new();
        for name in &present_functions {
            repo_contents.push_str("fn ");
            repo_contents.push_str(name);
            repo_contents.push_str("() {}\n");
        }
        for name in &present_symbols {
            repo_contents.push_str("struct ");
            repo_contents.push_str(name);
            repo_contents.push_str(";\n");
        }

        let dir = TempDir::new("resolver-batch");
        init_repo_with_file(&dir, "code.rs", &repo_contents);
        let tracked = HashSet::new();
        let resolver = Resolver {
            root: &dir,
            tracked: &tracked,
            baseline_sha: None,
        };

        let resolution = resolver.resolve(&anchors);
        let mut expected_unresolved: Vec<UnresolvedAnchor> = function_names
            .iter()
            .filter(|name| !present_functions.contains(*name))
            .map(|name| UnresolvedAnchor {
                anchor: name.clone(),
                category: "Function".to_string(),
                reason: "not first-party".to_string(),
            })
            .collect();
        expected_unresolved.sort_by(|a, b| a.anchor.cmp(&b.anchor));
        let mut expected_broken: Vec<BrokenAnchor> = symbol_names
            .iter()
            .filter(|name| !present_symbols.contains(*name))
            .map(|name| BrokenAnchor {
                anchor: name.clone(),
                category: "Symbol".to_string(),
                reason: "symbol not found in repo".to_string(),
            })
            .collect();
        expected_broken.sort_by(|a, b| a.anchor.cmp(&b.anchor));

        assert_eq!(resolution.broken.len(), expected_broken.len());
        for (actual, expected) in resolution.broken.iter().zip(expected_broken.iter()) {
            assert_eq!(actual.anchor, expected.anchor);
            assert_eq!(actual.category, expected.category);
            assert_eq!(actual.reason, expected.reason);
        }
        assert_eq!(resolution.unresolved.len(), expected_unresolved.len());
        for (actual, expected) in resolution.unresolved.iter().zip(expected_unresolved.iter()) {
            assert_eq!(actual.anchor, expected.anchor);
            assert_eq!(actual.category, expected.category);
            assert_eq!(actual.reason, expected.reason);
        }
    }
}
