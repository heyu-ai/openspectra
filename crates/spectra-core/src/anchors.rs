//! Design anchors: references in `design.md` to concrete code artifacts that
//! drift can verify still resolve against the current codebase.
//!
//! The extraction regexes and the original 42-entry core of the symbol
//! stop-list were recovered from the reference binary's `.rodata`.
//! Probe-derived corrections are documented at their definitions:
//!
//! * `FILE_PATH_RE` is **not** the `.rodata` regex — it adds a prefix head, a
//!   left-boundary check, and a `..` guard so a reported anchor exists in the
//!   design and denotes a path inside the project (#123).
//!   `ORACLE_FILE_PATH_RE` keeps the verbatim form, and `path_candidates`
//!   resolves against the union of both, which is deliberately more permissive
//!   than the oracle — see its doc comment.
//! * `SYMBOL_STOPLIST` has 21 probe-recovered entries beyond the `.rodata`
//!   core: `JSON` plus the 20 additions from the 2026-08-06 sweep (#133).

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
/// `src/services/apiClient.ts` and reports "file does not exist" for a string
/// that appears nowhere in the design. Probed against v2.3.1 (2026-08-03) with
/// `frontend/src/services/apiClient.ts` present on disk, the oracle still
/// reports it broken, and it resolves only when the *stripped* path exists — so
/// this is a faithful port of an oracle defect, not a porting error. On the
/// reporting corpus all six surviving broken anchors were this false positive.
///
/// Three extraction corrections:
/// * the `(?:[\w.-]+/)*` head captures leading path segments, so a monorepo
///   sub-project path is reported verbatim;
/// * `starts_at_path_boundary` rejects a match that begins mid-token, so
///   `mysrc/foo.rs` yields no anchor instead of a phantom `src/foo.rs`;
/// * `escapes_project_root` drops a path with a `..` segment, which the head
///   would otherwise let resolve against a file outside the project.
///
/// Together they make every reported FilePath anchor greppable verbatim in the
/// source design, which is what makes the finding actionable. Resolution is
/// separately widened — see `path_candidates`.
static FILE_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:[\w.-]+/)*(?:src-tauri|src|crates|docs)/[\w./-]+\.(?:rs|ts|svelte|md|toml)")
        .unwrap()
});

/// The recovered `.rodata` regex, without `FILE_PATH_RE`'s prefix head.
///
/// Kept so resolution can still consult the form the oracle would have used
/// (see `path_candidates`), which is what stops the #123 divergence from
/// regressing a project rooted at its own sub-project.
static ORACLE_FILE_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:src-tauri|src|crates|docs)/[\w./-]+\.(?:rs|ts|svelte|md|toml)").unwrap()
});

/// On-disk forms to try for a FilePath anchor: the path as written, plus the
/// oracle's truncated form when it differs.
///
/// Without the union a project rooted at its own sub-project regresses: a design
/// under `frontend/` citing the monorepo-relative `frontend/src/x.ts` is looked
/// up at `frontend/frontend/src/x.ts` and reported broken, where the oracle's
/// truncated `src/x.ts` resolves. Probed: oracle `0/1`, first draft of this fix
/// `1/1`.
///
/// **This widens resolution; it is not a text-only change.** A path resolves
/// when *either* form is present, where the oracle consults only the truncation.
/// Probed in the mirror case — design cites `frontend/src/services/apiClient.ts`
/// and that exact file exists — the oracle reports it broken and this resolves
/// it. That is the accepted trade: #123's broken-FilePath class measured a 0/6
/// true-positive rate, so a common false positive is exchanged for a rarer false
/// negative.
///
/// `Resolver::resolve` applies the same union to the `.started` baseline probe,
/// deliberately: the two questions must be asked with one resolution rule, or
/// "resolved then, not now" stops meaning anything. The consequence is that the
/// union **can** move an anchor from `unresolved / forward reference` to
/// `broken` — an earlier revision of this comment claimed it could not, which
/// was false. Both directions are probed (2026-08-03):
///
/// * nested root, cited file really deleted — union yields `broken`; a
///   written-path-only baseline probe would call it a forward reference, a false
///   negative in exactly the layout this fallback exists for;
/// * repo root, `frontend/src/x.ts` never existed but an unrelated `src/x.ts`
///   was deleted — union yields `broken`, a false positive.
///
/// The two are indistinguishable from the anchor text alone, so the false
/// positive is accepted as the same coincidental-truncation *cause* already
/// accepted for the present-day probe. The directions differ, though: the
/// present-day union only suppresses findings, while this one manufactures one —
/// the direction a merge gate feels.
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

/// Tokens the oracle excludes from Symbol anchors.
///
/// The original 42-entry core was recovered from `.rodata`. `JSON` was added by
/// a per-token v2.3.1 probe on 2026-08-03 (#51). A fresh-jail sweep on
/// 2026-08-06 added 20 more entries (#133), spanning Rust standard-library
/// types and traits, Rust keywords and idiom, the `Markdown` and `Rust` format
/// or language names, and the Gherkin words `When` and `Then` (`Given` was
/// already in the core).
///
/// This is an empirical set, not a licence to complete semantic families. The
/// 2026-08-06 sweep found that all tested common English prose words, including
/// `This`, `We`, `That`, and `And`, are extracted by the oracle. It also found
/// adjacent Rust, format, and framework tokens that remain extracted; see the
/// recorded probe in `docs/reverse-engineering/drift.md`.
static SYMBOL_STOPLIST: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "Context", "State", "Result", "Error", "Option", "Vec", "Box", "String", "HashMap",
        "HashSet", "PathBuf", "Ok", "Err", "Default", "Sized", "Clone", "Debug", "Display", "Fn",
        "FnMut", "FnOnce", "Future", "Pin", "Arc", "Rc", "RefCell", "Mutex", "RwLock", "Trait",
        "Module", "Struct", "Field", "Method", "Value", "Source", "Target", "Given", "Spectra",
        "CLI", "GUI", "API", "IPC", "JSON",
        // Rust standard-library types and traits (#133 probe).
        "Copy", "Send", "Sync", "Drop", "From", "Into", "Iterator", "Cell", "Path",
        // Rust keywords and idiom (#133 probe).
        "Self", "Some", "None", "Enum", "Type", "Function", "Item",
        // Formats and languages (#133 probe).
        "Markdown", "Rust",
        // Gherkin (#133 probe; `Given` is already in the `.rodata` core).
        "When", "Then",
    ]
    .into_iter()
    .collect()
});

/// Whether a `FILE_PATH_RE` match at `start` begins at a real token boundary
/// rather than in the middle of a longer path or word.
///
/// The `regex` crate has no look-behind, so the boundary is checked here: a
/// preceding **ASCII** word or path byte (`mysrc/foo.rs`, `…/a/src/b.rs`
/// already consumed by the head group) means the match is a suffix of something
/// longer and must not become an anchor.
///
/// Deliberately ASCII-only, unlike the regex head's Unicode-aware `\w`: a
/// non-ASCII neighbour counts as a boundary, so CJK prose abutting a path
/// (`說明src/foo.rs`) still yields `src/foo.rs`. The cost is that a Latin-script
/// non-ASCII prefix is inconsistent — `über/src/x.rs` is consumed whole by the
/// head, but `Ωsrc/x.rs` still anchors at `src/x.rs`. Prose-abutted paths are
/// the common case in this corpus; both were measured.
///
/// `design` is indexed by byte, and `start` comes from the regex engine, so it
/// is always a UTF-8 boundary.
fn starts_at_path_boundary(design: &str, start: usize) -> bool {
    match design.as_bytes()[..start].last() {
        None => true,
        Some(&b) => !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-')),
    }
}

/// Whether `path` contains a `..` segment.
///
/// Such a path is dropped rather than anchored. Resolution joins the anchor onto
/// the project root, so `../src/main.rs` would stat the root's *parent* and let
/// an unrelated file outside the project silently satisfy a design reference.
/// Probed (2026-08-03) before this guard existed: with `<outer>/src/main.rs`
/// present and the project at `<outer>/proj`, the oracle reported `src/main.rs`
/// broken while this crate reported nothing at all.
///
/// Dropping matches how a leading `/` is already handled — an anchor that cannot
/// denote a path *inside* the project is not a checkable reference. `.` segments
/// are left alone: `./src/x.rs` stays within the root.
fn escapes_project_root(path: &str) -> bool {
    path.split('/').any(|segment| segment == "..")
}

/// Extract unique anchors from `design.md` text, deduped by string (first
/// matching category wins) and reduced by [`sample_over_cap`] when the set
/// exceeds [`ANCHOR_CAP`].
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
        if !starts_at_path_boundary(design, m.start()) || escapes_project_root(m.as_str()) {
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
    // The regex below is exactly what the binary's `.rodata` contains. The
    // stop-list combines its recovered core with the probe-recovered additions
    // documented at `SYMBOL_STOPLIST`.
    //
    // This used to carry a KNOWN DIVERGENCE note calling the oracle's apparent
    // "Symbol narrowing" (12 of ~83 candidates in one prose-heavy design) an
    // undetermined predicate that resisted black-box probing — the repo's single
    // open RE question (#8/#51). It is not a semantic filter: it is
    // `sample_over_cap` below. Probed on v2.3.1 with bare `WidgetNN` tokens and
    // nothing else: 30 candidates (under the cap) keep all 30, and 83 keep
    // exactly 12 at indices `floor(i * 83 / 12)`. `12 of ~83` is
    // `ANCHOR_SAMPLE_PER_CATEGORY` of 83, and the old "keeps `Data`, drops
    // `Model`" observations were positional, not semantic. See
    // docs/reverse-engineering/drift.md.
    for m in SYMBOL_RE.find_iter(design) {
        let s = m.as_str();
        if SYMBOL_STOPLIST.contains(s) {
            continue;
        }
        push(s.to_string(), AnchorKind::Symbol, &mut seen, &mut out);
    }

    sample_over_cap(out)
}

/// Reduce an over-cap anchor set the way the oracle does.
///
/// Sets of [`ANCHOR_CAP`] or fewer anchors are checked whole. Larger sets are
/// *not* truncated to the cap: each category independently keeps an evenly
/// spaced sample of at most [`ANCHOR_SAMPLE_PER_CATEGORY`] anchors, taking
/// index `i * n / 12` for `i` in `0..12`. A category holding 12 or fewer anchors
/// is kept whole, so the denominator above the cap is 12 per over-cap category
/// plus the full count of each under-cap one — 12/24/36 when every present
/// category is over the sample size, but 17 for 60 FilePath + 5 Function. It is
/// never a number between 51 and the raw extracted count.
///
/// Pinned by probe against the v2.3.1 oracle: 53 and 77 pure-FilePath anchors
/// both reproduce the `i * n / 12` index set exactly; a design mixing 20
/// FilePath, 20 Function and 20 CliFlag anchors (60 total) yields 36, not 12,
/// so the sample is per category while the trigger is the combined total.
///
/// This is also the mechanism behind what was long recorded as the oracle's
/// unexplained Symbol-narrowing filter (#8/#51) — see the note in `extract`.
fn sample_over_cap(anchors: Vec<Anchor>) -> Vec<Anchor> {
    if anchors.len() <= ANCHOR_CAP {
        return anchors;
    }
    let mut kept = Vec::new();
    // Category order matches the extraction order above, so grouping preserves
    // the original relative order of the surviving anchors.
    for kind in [
        AnchorKind::FilePath,
        AnchorKind::CliFlag,
        AnchorKind::Function,
        AnchorKind::Symbol,
    ] {
        let group: Vec<&Anchor> = anchors.iter().filter(|a| a.kind == kind).collect();
        if group.len() <= ANCHOR_SAMPLE_PER_CATEGORY {
            kept.extend(group.into_iter().cloned());
        } else {
            for i in 0..ANCHOR_SAMPLE_PER_CATEGORY {
                kept.push(group[i * group.len() / ANCHOR_SAMPLE_PER_CATEGORY].clone());
            }
        }
    }
    kept
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
                        broken.push(BrokenAnchor {
                            anchor: anchor.text.clone(),
                            category,
                            reason: "function not found in repo".to_string(),
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
                AnchorKind::CliFlag => broken.push(BrokenAnchor {
                    anchor: anchor.text.clone(),
                    category,
                    reason: "not in --help".to_string(),
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

    /// #123: a path under a monorepo sub-project keeps its leading segments, so
    /// the reported anchor is greppable verbatim in the design that produced it.
    /// The oracle reports the truncated `src/...` here; this is a deliberate,
    /// probe-documented divergence (see `FILE_PATH_RE`).
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

    /// #123, the other half: a match starting mid-token is not an anchor at all.
    /// Without the boundary check `mysrc/foo.rs` yields a phantom `src/foo.rs`
    /// that appears nowhere in the source document.
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

    /// A `..` segment would let resolution stat outside the project root, so
    /// such a path is dropped instead of anchored. Probed before the guard
    /// existed: an unrelated `<outer>/src/main.rs` silently satisfied a design
    /// under `<outer>/proj` citing `../src/main.rs`. `./` stays inside the root
    /// and is kept.
    #[test]
    fn file_paths_reject_segments_that_escape_the_project_root() {
        assert!(kinds("see ../src/main.rs here", AnchorKind::FilePath).is_empty());
        assert!(kinds("see a/../../src/main.rs here", AnchorKind::FilePath).is_empty());
        assert_eq!(
            kinds("see ./src/main.rs here", AnchorKind::FilePath),
            vec!["./src/main.rs"]
        );
    }

    /// #51: a freshly scaffolded `design.md` must not manufacture broken Symbol
    /// anchors from its own template prose. `JSON` was the last word the oracle
    /// stop-lists and this list did not.
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
    fn symbol_stoplist_excludes_common_types() {
        let d = "Result and Option and MyType here";
        assert_eq!(kinds(d, AnchorKind::Symbol), vec!["MyType"]);
    }

    /// #133: the 2026-08-06 fresh-jail sweep found these 20 additional tokens
    /// absent from the oracle's Symbol `broken_anchors`. The sentinel confirms
    /// that ordinary candidates in the same under-cap input are still extracted.
    #[test]
    fn probe_recovered_symbol_stoplist_entries_match_the_oracle() {
        let d = "Copy Send Sync Drop From Into Iterator Cell Path Self Some None Enum Type \
                 Function Item Markdown Rust When Then Kumquat";
        assert_eq!(kinds(d, AnchorKind::Symbol), vec!["Kumquat"]);
        assert_eq!(SYMBOL_STOPLIST.len(), 63);
    }

    #[test]
    fn dedup_keeps_one_anchor_per_distinct_string() {
        let d = "--flag --flag --flag";
        assert_eq!(extract(d).len(), 1);
    }

    #[test]
    fn sets_at_the_cap_are_kept_whole() {
        let at_cap = (0..ANCHOR_CAP)
            .map(|i| format!("--flag-{i:03}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(extract(&at_cap).len(), ANCHOR_CAP);
    }

    #[test]
    fn over_cap_sets_are_sampled_evenly_per_category() {
        // One category over the cap: the denominator drops to the per-category
        // sample size, not to ANCHOR_CAP.
        let flags: Vec<String> = (0..53).map(|i| format!("--flag-{i:03}")).collect();
        let anchors = extract(&flags.join(" "));
        assert_eq!(anchors.len(), ANCHOR_SAMPLE_PER_CATEGORY);
        let kept: Vec<&str> = anchors.iter().map(|a| a.text.as_str()).collect();
        let expected: Vec<&str> = (0..ANCHOR_SAMPLE_PER_CATEGORY)
            .map(|i| flags[i * 53 / ANCHOR_SAMPLE_PER_CATEGORY].as_str())
            .collect();
        assert_eq!(kept, expected);
    }

    #[test]
    fn over_cap_sampling_is_per_category_not_global() {
        // 26 CliFlags + 26 FilePaths trips the cap on the combined total while
        // each category stays above the sample size, so both are sampled to 12.
        let mut design = String::new();
        for i in 0..26 {
            design.push_str(&format!("--flag-{i:03} src/mod_{i:03}.rs "));
        }
        let anchors = extract(&design);
        assert_eq!(anchors.len(), 2 * ANCHOR_SAMPLE_PER_CATEGORY);
        let flags = anchors
            .iter()
            .filter(|a| a.kind == AnchorKind::CliFlag)
            .count();
        assert_eq!(flags, ANCHOR_SAMPLE_PER_CATEGORY);
    }

    #[test]
    fn over_cap_categories_under_the_sample_size_are_kept_whole() {
        let mut design = String::new();
        for i in 0..60 {
            design.push_str(&format!("--flag-{i:03} "));
        }
        design.push_str("src/only.rs");
        let anchors = extract(&design);
        assert_eq!(anchors.len(), ANCHOR_SAMPLE_PER_CATEGORY + 1);
        assert_eq!(
            anchors
                .iter()
                .filter(|a| a.kind == AnchorKind::FilePath)
                .count(),
            1
        );
    }

    #[test]
    fn cli_flags_are_broken_when_no_target_help_can_confirm_them() {
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
        assert!(resolution.unresolved.is_empty());
        assert_eq!(resolution.broken.len(), 1);
        assert_eq!(resolution.broken[0].reason, "not in --help");
        assert_eq!(resolution.broken[0].category, "CliFlag");
    }

    /// #123 must not trade one false positive for another. When the project root
    /// *is* the sub-project, a monorepo-relative citation still has to resolve —
    /// the oracle's truncated form is tried as well, so the reported text
    /// changes but the resolved/broken verdict does not.
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

    /// The other direction of the union, complementing
    /// `monorepo_path_resolves_against_the_oracle_truncation`: it must also
    /// resolve when only the path **as written** exists. Without this, a mutation
    /// that consulted only the truncation would pass the rest of the suite.
    #[test]
    fn monorepo_path_resolves_when_only_the_written_form_exists() {
        let dir = TempDir::new("written-form-only");
        std::fs::create_dir_all(dir.join("frontend/src/services")).unwrap();
        std::fs::write(
            dir.join("frontend/src/services/apiClient.ts"),
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
            "written form exists on disk, must resolve; got {:?}",
            resolution.broken
        );
    }

    /// The union is applied to the `.started` baseline probe as well as the
    /// present-day one, so an anchor whose *truncated* form resolved at the
    /// baseline is `broken`, not `forward reference`. This is deliberate — the
    /// alternative misses a real nested-root deletion — and it is the behaviour
    /// an earlier doc comment wrongly claimed could not happen, so pin it.
    #[test]
    fn baseline_probe_uses_the_same_union_as_the_present_day_probe() {
        let dir = TempDir::new("baseline-union");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        init_repo_with_file(&dir, "src/x.ts", "export const x = 1;\n");
        let baseline = std::process::Command::new("git")
            .arg("-C")
            .arg(&*dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let baseline = String::from_utf8(baseline.stdout)
            .unwrap()
            .trim()
            .to_string();
        // The truncation existed at the baseline and is now gone; the written
        // form never existed in either state.
        std::fs::remove_file(dir.join("src/x.ts")).unwrap();
        let tracked = HashSet::new();
        let resolver = Resolver {
            root: &dir,
            tracked: &tracked,
            baseline_sha: Some(&baseline),
        };

        let resolution = resolver.resolve(&extract("see `frontend/src/x.ts`"));

        assert_eq!(resolution.broken.len(), 1, "{resolution:?}");
        assert_eq!(resolution.broken[0].anchor, "frontend/src/x.ts");
        assert_eq!(resolution.broken[0].reason, "file does not exist");
        assert!(
            resolution.unresolved.is_empty(),
            "the truncation resolved at the baseline, so this is not a forward \
             reference; got {:?}",
            resolution.unresolved
        );
    }

    /// The union must not hide a genuine deletion: when neither the path as
    /// written nor its truncation exists, the anchor is still broken — and it is
    /// reported under the text the design actually contains.
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
        let mut expected_broken: Vec<BrokenAnchor> = function_names
            .iter()
            .filter(|name| !present_functions.contains(*name))
            .map(|name| BrokenAnchor {
                anchor: name.clone(),
                category: "Function".to_string(),
                reason: "function not found in repo".to_string(),
            })
            .chain(
                symbol_names
                    .iter()
                    .filter(|name| !present_symbols.contains(*name))
                    .map(|name| BrokenAnchor {
                        anchor: name.clone(),
                        category: "Symbol".to_string(),
                        reason: "symbol not found in repo".to_string(),
                    }),
            )
            .collect();
        expected_broken.sort_by(|a, b| a.anchor.cmp(&b.anchor));

        assert_eq!(resolution.broken.len(), expected_broken.len());
        for (actual, expected) in resolution.broken.iter().zip(expected_broken.iter()) {
            assert_eq!(actual.anchor, expected.anchor);
            assert_eq!(actual.category, expected.category);
            assert_eq!(actual.reason, expected.reason);
        }
        assert!(resolution.unresolved.is_empty());
    }
}
