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
}

impl Resolver<'_> {
    /// Return the failure reason if `anchor` no longer resolves, else `None`.
    fn broken_reason(&self, anchor: &Anchor) -> Option<&'static str> {
        match anchor.kind {
            AnchorKind::FilePath => {
                let on_disk = self.root.join(&anchor.text).exists();
                if self.tracked.contains(&anchor.text) || on_disk {
                    None
                } else {
                    Some("file does not exist")
                }
            }
            AnchorKind::Function => {
                if git::grep_exists(self.root, &anchor.text) {
                    None
                } else {
                    Some("function not found in repo")
                }
            }
            AnchorKind::Symbol => {
                if git::grep_exists(self.root, &anchor.text) {
                    None
                } else {
                    Some("symbol not found in repo")
                }
            }
            // CliFlag verification requires a `--help` to diff against, which is
            // unavailable for an arbitrary target project. The reference binary
            // therefore reports every design.md flag as broken ("not in --help").
            // We reproduce that behavior faithfully.
            // TODO(calibration): improve by extracting flags only from fenced
            // code blocks or verifying against a configurable target CLI.
            AnchorKind::CliFlag => Some("not in --help"),
        }
    }

    /// Compute the broken anchors among `anchors`, sorted by anchor string
    /// (matching the reference output ordering).
    pub fn broken(&self, anchors: &[Anchor]) -> Vec<BrokenAnchor> {
        let mut broken: Vec<BrokenAnchor> = anchors
            .iter()
            .filter_map(|a| {
                self.broken_reason(a).map(|reason| BrokenAnchor {
                    anchor: a.text.clone(),
                    category: a.kind.as_str().to_string(),
                    reason: reason.to_string(),
                })
            })
            .collect();
        broken.sort_by(|a, b| a.anchor.cmp(&b.anchor));
        broken
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn cli_flags_are_always_broken_not_in_help() {
        use std::collections::HashSet;
        let tracked: HashSet<String> = HashSet::new();
        let root = std::path::Path::new(".");
        let r = Resolver {
            root,
            tracked: &tracked,
        };
        let anchors = extract("uses --some-flag");
        let broken = r.broken(&anchors);
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].reason, "not in --help");
        assert_eq!(broken[0].category, "CliFlag");
    }
}
