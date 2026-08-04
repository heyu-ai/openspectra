//! Design anchors: references in `design.md` to concrete code artifacts that
//! drift can verify still resolve against the current codebase.
//!
//! The four extraction regexes and the symbol stop-list are recovered from the
//! reference binary's `.rodata`. Three of the regexes are verbatim; two
//! probe-derived corrections are documented at their definitions:
//!
//! * `FILE_PATH_RE` is **not** the `.rodata` regex — it adds a prefix head and
//!   a left-boundary check so a reported anchor exists in the design (#123).
//!   `ORACLE_FILE_PATH_RE` keeps the verbatim form, and `path_candidates`
//!   resolves against both, so the divergence is confined to the reported text.
//! * `JSON` in `SYMBOL_STOPLIST` was recovered by probe, not from `.rodata`.
//!
//! The anchor budget in `apply_anchor_budget` is likewise probe-recovered
//! rather than read out of strings.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

use crate::calibration::{ANCHOR_CAP, ANCHOR_SAMPLE_PER_CATEGORY};
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
/// FilePath candidates.
///
/// DELIBERATE DIVERGENCE (#123). The recovered `.rodata` regex is the
/// `(?:src-tauri|src|crates|docs)/…` part alone, with no left boundary, so it
/// matches from the middle of a longer path: the oracle turns
/// `frontend/src/services/apiClient.ts` into the anchor
/// `src/services/apiClient.ts` and then reports "file does not exist" for a
/// string that appears nowhere in the design. Probed against v2.3.1
/// (2026-08-03): with `frontend/src/services/apiClient.ts` present on disk the
/// oracle still reports it broken, and it resolves only when the *stripped*
/// `src/services/apiClient.ts` exists — so the oracle really does anchor on the
/// truncated path, and this is a faithful port of an oracle defect rather than
/// a porting error.
///
/// Two corrections, both scoped to extraction:
/// * the `(?:[\w.-]+/)*` head captures leading path segments, so a monorepo
///   sub-project path is reported (and resolved) verbatim;
/// * [`starts_at_path_boundary`] rejects a match that begins mid-token, so
///   `mysrc/foo.rs` yields no anchor instead of a phantom `src/foo.rs`.
///
/// Together they make every reported FilePath anchor greppable verbatim in the
/// source design, which is what makes the finding actionable.
static FILE_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:[\w.-]+/)*(?:src-tauri|src|crates|docs)/[\w./-]+\.(?:rs|ts|svelte|md|toml)")
        .unwrap()
});

/// The recovered `.rodata` regex, without [`FILE_PATH_RE`]'s prefix head.
///
/// Kept because the divergence is confined to the *reported* anchor text: a
/// path is still resolved against the oracle's truncated form as well as the
/// text as written (see [`path_candidates`]).
static ORACLE_FILE_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:src-tauri|src|crates|docs)/[\w./-]+\.(?:rs|ts|svelte|md|toml)").unwrap()
});

/// On-disk forms to try for a FilePath anchor: the path as written, plus the
/// oracle's truncated form when it differs.
///
/// Reporting the full path (#123) must not change *which* paths resolve, only
/// what the finding is called. Without the fallback, a project rooted at the
/// sub-project itself regresses: a design under `frontend/` citing the
/// monorepo-relative `frontend/src/x.ts` would be looked up at
/// `frontend/frontend/src/x.ts` and reported broken, where the oracle's
/// truncated `src/x.ts` resolves. Probed against v2.3.1 (2026-08-03): oracle
/// `0/1`, pre-fallback OpenSpectra `1/1` — a false positive this fix removes.
///
/// The fallback only ever makes resolution more permissive, so it cannot invent
/// a broken anchor; at worst it matches the oracle's own behaviour.
fn path_candidates(anchor: &str) -> Vec<&str> {
    let mut candidates = vec![anchor];
    if let Some(m) = ORACLE_FILE_PATH_RE.find(anchor) {
        if m.as_str() != anchor {
            candidates.push(m.as_str());
        }
    }
    candidates
}
static CLI_FLAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"--[a-z][a-z0-9-]+").unwrap());
static SNAKE_FN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([a-z][a-z0-9]*_[a-z0-9_]+)\(").unwrap());
static CAMEL_FN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([a-z][a-z0-9]*[A-Z][a-zA-Z0-9]+)\(").unwrap());
static SYMBOL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[A-Z][a-zA-Z0-9]+\b").unwrap());

/// Common Rust types / framework words excluded from Symbol anchors so the
/// extractor does not flag ubiquitous identifiers as "drift" (recovered list).
///
/// `JSON` is the one entry not read out of `.rodata`: the oracle drops it and
/// this list did not, which was the whole remaining divergence on a fresh
/// `design.md` scaffold (#51). Probed per-token against v2.3.1 (2026-08-03) —
/// each candidate in its own repo alongside an unresolvable control symbol —
/// and `JSON` was the only word in the scaffold dropped by the oracle and kept
/// here. The probe also refuted the "all-caps acronyms are dropped" theory:
/// `ALTER`, `TABLE`, `COLUMN`, `README`, `CRITICAL`, `YAML`, `HTTP` and `SQL`
/// are all kept by the oracle in isolation.
static SYMBOL_STOPLIST: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "Context", "State", "Result", "Error", "Option", "Vec", "Box", "String", "HashMap",
        "HashSet", "PathBuf", "Ok", "Err", "Default", "Sized", "Clone", "Debug", "Display", "Fn",
        "FnMut", "FnOnce", "Future", "Pin", "Arc", "Rc", "RefCell", "Mutex", "RwLock", "Trait",
        "Module", "Struct", "Field", "Method", "Value", "Source", "Target", "Given", "Spectra",
        "CLI", "GUI", "API", "IPC", "JSON",
    ]
    .into_iter()
    .collect()
});

/// Whether a [`FILE_PATH_RE`] match at `start` begins at a real token boundary
/// rather than in the middle of a longer path or word.
///
/// The `regex` crate has no look-behind, so the boundary is checked here: any
/// preceding path or word byte (`mysrc/foo.rs`, `…/a/src/b.rs` already consumed
/// by the head group) means the match is a suffix of something longer and must
/// not become an anchor. `design` is indexed by byte, and `start` comes from
/// the regex engine, so it is always a UTF-8 boundary.
fn starts_at_path_boundary(design: &str, start: usize) -> bool {
    match design.as_bytes()[..start].last() {
        None => true,
        Some(&b) => !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-')),
    }
}

/// Reduce the candidate set to the anchors the oracle actually checks.
///
/// Recovered by probe against v2.3.1 (2026-08-03), replacing the earlier
/// `truncate(ANCHOR_CAP)` guess: the cap is a *trigger*, not a length. At or
/// below [`ANCHOR_CAP`] candidates every anchor is checked; above it each
/// category is independently downsampled to at most
/// [`ANCHOR_SAMPLE_PER_CATEGORY`] anchors, evenly spaced over that category's
/// document-order list at indices `i * n / 12`. A category with fewer than 12
/// candidates survives whole, so the reported total above the cap is
/// `sum(min(n_category, 12))` — the 12–21 range seen in oracle output, and the
/// long-open "12 of ~83 symbols" question (#8).
///
/// Extraction order is preserved, so `Function` covers the snake-case matches
/// followed by the camel-case ones — verified to be the oracle's own intra-
/// category order by sampling an interleaved 15+15 document.
fn apply_anchor_budget(candidates: Vec<Anchor>) -> Vec<Anchor> {
    if candidates.len() <= ANCHOR_CAP {
        return candidates;
    }
    let mut keep = vec![false; candidates.len()];
    for kind in [
        AnchorKind::FilePath,
        AnchorKind::CliFlag,
        AnchorKind::Function,
        AnchorKind::Symbol,
    ] {
        let positions: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, anchor)| anchor.kind == kind)
            .map(|(index, _)| index)
            .collect();
        let n = positions.len();
        if n == 0 {
            continue;
        }
        for i in 0..ANCHOR_SAMPLE_PER_CATEGORY {
            keep[positions[i * n / ANCHOR_SAMPLE_PER_CATEGORY]] = true;
        }
    }
    candidates
        .into_iter()
        .zip(keep)
        .filter_map(|(anchor, keep)| keep.then_some(anchor))
        .collect()
}

/// Extract unique anchors from `design.md` text, deduped by string (first
/// matching category wins) and reduced by `apply_anchor_budget`.
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
        if !starts_at_path_boundary(design, m.start()) {
            continue;
        }
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
    // The "Symbol narrowing filter" that used to be flagged here as the single
    // open reverse-engineering question was not a Symbol rule at all: the
    // oracle keeps every regex match until the *combined* candidate count
    // exceeds ANCHOR_CAP, then downsamples every category positionally. That is
    // why the same token was kept in one document and dropped in another under
    // identical local context. See `apply_anchor_budget`.
    for m in SYMBOL_RE.find_iter(design) {
        let s = m.as_str();
        if SYMBOL_STOPLIST.contains(s) {
            continue;
        }
        push(s.to_string(), AnchorKind::Symbol, &mut seen, &mut out);
    }

    apply_anchor_budget(out)
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
                    let candidates = path_candidates(&anchor.text);
                    let present = candidates
                        .iter()
                        .any(|p| self.tracked.contains(*p) || self.root.join(p).exists());
                    if present {
                        continue;
                    }
                    // `forward reference` requires every candidate form to be
                    // definitely absent at the baseline. One `Some(true)` means
                    // the path was there and is now gone (broken); one `None`
                    // means the baseline is unusable, which must not be read as
                    // proof of absence.
                    let existed_at_baseline = self.baseline_sha.and_then(|sha| {
                        candidates
                            .iter()
                            .map(|p| git::path_exists_at(self.root, sha, p))
                            .try_fold(false, |acc, seen| Some(acc || seen?))
                    });
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
    fn dedup_by_string() {
        let d = "--flag --flag --flag";
        assert_eq!(extract(d).len(), 1);
    }

    /// The candidate total, not any single category, decides whether the
    /// budget applies — and at the boundary nothing is dropped.
    /// Oracle-probed: 50 candidates -> 50/50, 51 -> 12/12.
    #[test]
    fn anchor_budget_triggers_just_above_the_cap() {
        let flags = |n: usize| {
            (0..n)
                .map(|i| format!("--flag{i:03}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(extract(&flags(ANCHOR_CAP)).len(), ANCHOR_CAP);
        assert_eq!(
            extract(&flags(ANCHOR_CAP + 1)).len(),
            ANCHOR_SAMPLE_PER_CATEGORY
        );
    }

    /// Above the cap the surviving anchors are evenly spaced at `i * n / 12`,
    /// not the first 12. Both expectations below are the oracle's verbatim
    /// output for these documents (v2.3.1, probed 2026-08-03).
    #[test]
    fn anchor_budget_samples_evenly_over_document_order() {
        let flags = |n: usize| {
            (0..n)
                .map(|i| format!("--flag{i:03}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let names = |design: &str| {
            extract(design)
                .into_iter()
                .map(|a| a.text)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            names(&flags(51)),
            [
                "--flag000",
                "--flag004",
                "--flag008",
                "--flag012",
                "--flag017",
                "--flag021",
                "--flag025",
                "--flag029",
                "--flag034",
                "--flag038",
                "--flag042",
                "--flag046",
            ]
        );
        assert_eq!(
            names(&flags(100)),
            [
                "--flag000",
                "--flag008",
                "--flag016",
                "--flag025",
                "--flag033",
                "--flag041",
                "--flag050",
                "--flag058",
                "--flag066",
                "--flag075",
                "--flag083",
                "--flag091",
            ]
        );
    }

    /// Each category gets its own budget, so a small category survives whole
    /// while a large one is sampled: the oracle's totals above the cap are
    /// `sum(min(n_category, 12))`, which is why they land in the 12–21 range
    /// rather than at 50 (#119).
    #[test]
    fn anchor_budget_is_per_category_and_spares_small_categories() {
        let mut design = String::from("# design\n\n");
        for i in 0..45 {
            design.push_str(&format!("Zsym{i:03} "));
        }
        for i in 0..10 {
            design.push_str(&format!("--flag{i:03} "));
        }
        let anchors = extract(&design);

        let count = |kind| anchors.iter().filter(|a| a.kind == kind).count();
        assert_eq!(count(AnchorKind::Symbol), ANCHOR_SAMPLE_PER_CATEGORY);
        assert_eq!(count(AnchorKind::CliFlag), 10, "small category kept whole");
        assert_eq!(anchors.len(), 22);
    }

    /// #123: a path under a monorepo sub-project keeps its leading segments,
    /// so the reported anchor is greppable verbatim in the design that
    /// produced it. The oracle reports the truncated `src/...` here; this is a
    /// deliberate, probe-documented divergence (see `FILE_PATH_RE`).
    #[test]
    fn file_paths_keep_leading_path_segments() {
        let d = "see `frontend/src/services/apiClient.ts` and `heyuai/docs/yibi/e02.md`";
        assert_eq!(
            kinds(d, AnchorKind::FilePath),
            vec![
                "frontend/src/services/apiClient.ts",
                "heyuai/docs/yibi/e02.md"
            ]
        );
    }

    /// #123 must not trade one false positive for another. When the project
    /// root *is* the sub-project, a monorepo-relative citation still has to
    /// resolve — the oracle's truncated form is tried as a fallback, so the
    /// reported text changes but the resolved/broken verdict does not.
    /// Probe: oracle `0/1`, OpenSpectra without this fallback `1/1`.
    #[test]
    fn monorepo_path_resolves_against_the_oracle_truncation() {
        let dir = TempDir::new("nested-root");
        std::fs::create_dir_all(dir.join("src/services")).unwrap();
        std::fs::write(
            dir.join("src/services/apiClient.ts"),
            "export const x = 1;\n",
        )
        .unwrap();
        let tracked = HashSet::new();
        let resolver = Resolver {
            root: &dir,
            tracked: &tracked,
            baseline_sha: None,
        };

        let resolution = resolver.resolve(&extract("see `frontend/src/services/apiClient.ts`"));

        assert!(
            resolution.broken.is_empty(),
            "monorepo-relative path must resolve via the oracle truncation, got {:?}",
            resolution.broken
        );
    }

    /// The fallback must not hide a genuine deletion: when neither the path as
    /// written nor its truncation exists, the anchor is still broken — and it
    /// is reported under the text the design actually contains.
    #[test]
    fn monorepo_path_still_breaks_when_neither_form_exists() {
        let tracked = HashSet::new();
        let dir = TempDir::new("nested-root-missing");
        let resolver = Resolver {
            root: &dir,
            tracked: &tracked,
            baseline_sha: None,
        };

        let resolution = resolver.resolve(&extract("see `frontend/src/services/gone.ts`"));

        assert_eq!(resolution.broken.len(), 1);
        assert_eq!(resolution.broken[0].anchor, "frontend/src/services/gone.ts");
    }

    /// #123, the other half: a match starting mid-token is not an anchor at
    /// all. Without the boundary check `mysrc/foo.rs` yields a phantom
    /// `src/foo.rs` that appears nowhere in the source document.
    #[test]
    fn file_paths_reject_matches_starting_mid_token() {
        assert!(kinds("see mysrc/foo.rs here", AnchorKind::FilePath).is_empty());
        assert!(kinds("see xdocs/a.md here", AnchorKind::FilePath).is_empty());
        // A boundary character before the root still anchors normally.
        assert_eq!(
            kinds("see (src/foo.rs) here", AnchorKind::FilePath),
            vec!["src/foo.rs"]
        );
    }

    /// #51: a freshly scaffolded `design.md` must not manufacture broken
    /// Symbol anchors from its own template prose. `JSON` was the last word
    /// the oracle stop-lists and this list did not.
    #[test]
    fn design_template_symbols_match_the_oracle_stoplist() {
        let symbols = kinds(crate::schema::DESIGN_TEMPLATE, AnchorKind::Symbol);
        assert!(
            !symbols.iter().any(|s| s == "JSON"),
            "JSON must be stop-listed; extracted symbols: {symbols:?}"
        );
        // Oracle-probed on the byte-identical scaffold: 20 anchors total.
        assert_eq!(extract(crate::schema::DESIGN_TEMPLATE).len(), 20);
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
