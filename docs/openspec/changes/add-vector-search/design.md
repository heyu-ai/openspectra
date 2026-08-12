# Design: dependency-free search

## Architecture

The CLI loads the project config and calls `spectra_core::search::search`.
Core recursively discovers `.md` files under `<spec_dir>`, reads each file as
one document, tokenizes the query and corpus, computes BM25 scores, sorts by
score and path, and applies `--limit`.

## Corpus

Scanning all Markdown files below `<spec_dir>` includes canonical specs,
active changes, and archived changes. Paths in results are relative to the
project root and use `/` separators. Containment is enforced, not assumed:
`<spec_dir>` is rejected if it is absolute or contains `..`, and every scanned
directory and file is canonicalized (following symlinks) and skipped unless it
resolves inside the canonicalized project root, so a symlinked `spec_dir` or a
symlinked `.md` cannot surface a file outside the project.

## Tokenization

- Non-CJK letters and numbers form lowercased word tokens.
- `_` remains part of a word token.
- CJK scripts form both unigram and adjacent bigram tokens. "CJK" here covers
  Han ideographs (Unified, Extension A, Extension B, and Compatibility
  Ideographs), Japanese kana (Hiragana, Katakana, and phonetic extensions), and
  Korean Hangul (Jamo, compatibility Jamo, and precomposed syllables).

The CJK representation permits a multi-character query to prefer the same
phrase without requiring a segmentation dictionary or new dependency, and lets
a shorter Japanese/Korean query match a longer unspaced phrase.

## Ranking and scores

BM25 uses `k1 = 1.2` and `b = 0.75`. Query term frequency contributes a
logarithmic weight. Raw BM25 scores are mapped with `s / (s + 1)` so callers
receive a finite value in `(0, 1)` while preserving ordering. Equal scores
sort by path for reproducibility.

The normalization is an OpenSpectra contract, not an oracle-derived cosine
score. It supports consumers that treat very small scores as weak matches
without pretending lexical relevance is cosine similarity.

## Snippets

Each result contains one whitespace-normalized snippet of at most 200 source
characters, beginning a quarter-window before the earliest literal query-token
match when one is available (so the match sits near the start of the window).
Leading or trailing omissions use an ellipsis.

## Failure behavior

Search requires an initialized project, matching the oracle. A missing or
empty spec directory is a successful empty result. Filesystem errors other
than a missing directory fail loudly.
