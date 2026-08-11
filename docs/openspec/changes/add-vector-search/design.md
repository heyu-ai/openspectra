# Design: dependency-free search

## Architecture

The CLI loads the project config and calls `spectra_core::search::search`.
Core recursively discovers `.md` files under `<spec_dir>`, reads each file as
one document, tokenizes the query and corpus, computes BM25 scores, sorts by
score and path, and applies `--limit`.

## Corpus

Scanning all Markdown files below `<spec_dir>` includes canonical specs,
active changes, and archived changes. Paths in results are relative to the
project root, use `/` separators, and therefore remain inside the configured
spec directory.

## Tokenization

- Non-CJK letters and numbers form lowercased word tokens.
- `_` remains part of a word token.
- CJK Unified Ideographs form both unigram and adjacent bigram tokens.

The CJK representation permits a multi-character query to prefer the same
phrase without requiring a segmentation dictionary or new dependency.

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
characters, centered near the earliest literal query-token match when one is
available. Leading or trailing omissions use an ellipsis.

## Failure behavior

Search requires an initialized project, matching the oracle. A missing or
empty spec directory is a successful empty result. Filesystem errors other
than a missing directory fail loudly.
