# Proposal: add lexical search

## Why

OpenSpectra lacks the oracle's `spectra search <QUERY>` interface. The oracle
uses a GUI-built hybrid index, but reproducing that architecture would add a
model download, ONNX runtime, and persistent-index lifecycle to a CLI that is
otherwise dependency-light.

Maintainer decision (2026-08-11): provide the same CLI and consumer-facing
result shape with dependency-free BM25-style lexical ranking. This is an
intentional ranking divergence, not a claim of semantic-search parity.

## What changes

- Add `spectra search <QUERY> [--limit N] [--json]`.
- Scan Markdown files below the configured spec directory at query time.
- Rank files with BM25 and deterministic tie-breaking.
- Tokenize CJK runs into unigrams and bigrams as well as ordinary words.
- Return `query`, `results[].path`, `results[].score`, and
  `results[].snippets` in JSON.
- Keep search offline and free of model, network, and persistent-index
  dependencies.

## Out of scope

- Vector embeddings, ONNX, Tantivy, RRF, and model provisioning.
- A `.vector-search.db` index or an index-management command.
- Ranking equivalence with the closed-source hybrid engine.
