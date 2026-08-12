# Reverse-engineering and implementing `spectra search`

How the closed-source `spectra search` command answers a natural-language
query over a project's spec artifacts, and what OpenSpectra would need to
reproduce it. This is the feature the GUI surfaces as「向量搜尋 / 向量模型 /
語意搜尋索引」.

> Source binary: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O,
> symbols + build paths retained → string mining was effective). Unlike
> `drift`, this behaviour was **not** run as a golden oracle: on the probe
> machine the embedding model was not downloaded (`~/Library/Application
> Support/openspec/` empty), so `search` could not be exercised end-to-end.
> Everything below is **string-mined from the binary**, marked measured vs
> inferred per claim.

## TL;DR

- There is **no `ask` subcommand** in v2.3.1. The semantic feature is
  `spectra search <QUERY>` — a compiled binary subcommand, **not** a
  `/spectra:*` Claude Code slash-command. (Measured: `spectra --help`.)
- `search` is **hybrid retrieval**: sparse **BM25 via `tantivy`** + dense
  **vector** embeddings, fused with **RRF (Reciprocal Rank Fusion)**. This is
  the concrete substance of the note in `drift.md` ("the `search` command is
  the one that uses vector/BM25 indexing"). (Measured: binary strings `bm25`,
  `tantivy`, `hybrid`, `reciprocal`, `rrf`, `cosine`, `dotproduct`,
  `normalize`, `rank`.)
- Dense embeddings come from **`intfloat/multilingual-e5-small`** in
  **quantized ONNX** form, run locally via **ONNX Runtime** (the `ort` crate)
  with the **`tokenizers`** crate. No network at query time; no external API.
  (Measured: strings `multilingual-e5-small-qonnx/model_quantized.onnx`
  `tokenizer.json` `config.json`; ort/tokenizers build paths.)
- The index is a local file, **`.vector-search.db`**, gitignored by
  `spectra init`. (Measured: `.gitignore/.vector-search.db` string fragment,
  cross-referenced in `task.md`.)
- OpenSpectra deliberately implements the same CLI with immediate,
  dependency-free BM25 search instead. It does not emit model/index errors and
  does not claim ranking parity with the hybrid oracle.

## CLI surface (measured)

```
spectra search <QUERY> [--limit <N>] [--json] [--no-color]
```

`spectra search --help` verbatim:

```
Search documents using vector semantic search

Usage: spectra search [OPTIONS] <QUERY>

Arguments:
  <QUERY>  Search query string

Options:
      --limit <LIMIT>  Maximum number of results [default: 10]
      --no-color       Disable colored output
      --json           Output as JSON
```

Notably there is **no** `index` / `embed` / `reindex` subcommand at the top
level (measured: full `--help` command list). Index construction is therefore
either lazy (built/updated on first `search`) or GUI-driven — the desktop app
exposes explicit「重建索引 / 刪除索引」buttons and「向量模型：已下載 / 刪除
模型」, which are the same binary's app mode. The binary does contain
`build index` / `reindex` / `indexing` / `chunk` strings (measured), so the
logic exists internally; how the CLI path triggers it is **inferred, not
observed**.

## Embedding model (measured)

| Property | Value | Evidence |
|----------|-------|----------|
| Model | `intfloat/multilingual-e5-small` | string `multilingual-e5-small-qonnx` |
| Format | quantized ONNX | `model_quantized.onnx` |
| Dimension | **384** | e5-small is 384-d; `embedding_dim` present |
| Tokenizer | HF `tokenizers` (`tokenizer.json`) | strings + `tokenizers-0.22.2` build path |
| Runtime | ONNX Runtime (`ort`), CoreML EP on Apple Silicon | `ort-artifacts`, `com.microsoft.Onnx`, CoreML provider paths |
| Asset layout | `multilingual-e5-small-qonnx/{model_quantized.onnx, tokenizer.json, config.json, special_tokens_map.json, tokenizer_config.json}` | concatenated path string |

**E5 prompt convention (measured: strings `query:` / `passage:`):** e5 models
require an asymmetric prefix — index text is embedded as `passage: <text>`,
the query as `query: <text>`. Reproducing search **must** apply these prefixes
or relevance collapses; this is a silent-correctness trap, not a nicety.

`multilingual-e5-small` (not the English-only MiniLM) is why the GUI indexes
and retrieves CJK spec content correctly.

### Where the model lives (inferred)

The probe machine had no downloaded model, so the exact parent directory was
not observed. The config path is `~/Library/Application Support/openspec/`
(measured: `spectra config path`); the model almost certainly lands under that
tree (or an OS cache dir) as `multilingual-e5-small-qonnx/…`. Download URL is
constructed at runtime — no plaintext model URL in the binary — but the model
is a **public Hugging Face model**, so a reimplementation can fetch the qonnx
variant directly and does not need the original's download path.

## Index & retrieval pipeline (measured architecture, inferred details)

1. **Corpus**: specs + archived changes (GUI label:「規格與已封存變更的語意
   搜尋索引」; the sample install showed 326 docs / 18.6 MB). What exactly
   counts as a "document" and the **chunking** granularity are inferred
   (strings `chunk` / `chunking` exist; the split rule was not observed).
2. **Sparse arm**: `tantivy` BM25 index over the same corpus.
3. **Dense arm**: e5-small embeddings (`passage:` prefixed), stored in
   `.vector-search.db`. Similarity is cosine / normalized dot-product
   (measured strings). Whether ANN (HNSW) or brute-force flat scan is used was
   **not** resolved — no `hnsw` string surfaced, so a flat cosine scan over a
   few hundred vectors is the likely (inferred) implementation.
4. **Fusion**: results from both arms combined via **RRF** (measured
   `reciprocal` / `rrf`), then top-`--limit` returned.
5. **Store**: single SQLite-family file `.vector-search.db` at the project
   root, gitignored.

## Why the hybrid architecture was deliberately **not** ported (context)

`drift.md:13-15` scopes `search` out of the drift reimplementation on purpose.
The reasons, now concrete:

- **Heavy runtime assets**: bundling/fetching a ~100 MB+ quantized ONNX model,
  linking ONNX Runtime, a tokenizer, `tantivy`, and a vector store — versus
  `drift`, which is pure git + filesystem + regex with zero external assets.
- **No clean oracle judgement**: `drift` calibrates against exact numeric
  golden scores. Hybrid semantic ranking output shifts with quantization
  details, chunking, and fusion constants; the only realistic oracle check is
  "same query → same top-N document *set* (and ideally order)", which is
  fuzzier and model-version-sensitive.
- **Cross-platform cost**: ONNX Runtime + model download complicate the "one
  static musl binary, only needs `git`" release story (roadmap Phase 3).

## Open RE questions (would need the model downloaded + oracle runs)

- Exact **chunking** rule (per-file? per-heading? token window?).
- **RRF k constant** and any arm weighting.
- Dense index structure (flat vs ANN) and the `.vector-search.db` schema.
- Whether the CLI `search` builds the index lazily or requires a prior GUI
  "重建索引".
- Result JSON shape of `spectra search --json` (not captured — model absent).

Until these are answered by running the oracle with the model present, any
reimplementation is calibration-blind and can only aim for "reasonable hybrid
search", not "byte-identical to v2.3.1".

## Reproducing the observable oracle contract (2026-08-11)

These probes used `/Users/doxa/.local/bin/spectra`, which resolves to the
Spectra.app v2.3.1 arm64 executable. Each case used a separate temporary
project jail except the limit enumeration, which made no filesystem changes
after a single initialization.

| Case | Exit | stdout | stderr |
|---|---:|---|---|
| Outside a project: `search needle --json` | 1 | empty | `Error: Not initialized. Run 'spectra init' to initialize.` + newline |
| Fresh initialized project: `search needle --json` | 0 | `{"error":"index_not_built","results":[]}` + newline | empty |
| `--limit 0`, `1`, or `usize::MAX` without an index | 0 | same `index_not_built` JSON | empty |
| `--limit usize::MAX + 1` | 2 | empty | clap number-too-large error |
| `--limit nope` | 2 | empty | clap invalid-digit error |

The empty JSON is compact and exactly 41 bytes including its LF terminator.
The limit accepts zero and is parsed as a platform `usize`.

Strings in the binary additionally expose the result keys `query`, `results`,
`path`, `score`, and plural `snippets`, plus the human messages `No results
found.` and `Found N results for "..."`. A successful oracle result remains
unobservable on this machine because the desktop-built index is absent, so
the nesting and field order of a populated response remain inferred.

## OpenSpectra implementation decision (2026-08-11)

The maintainer selected a dependency-free lexical substitute for issue #63.
OpenSpectra recursively scans Markdown files below `<spec_dir>` on every
query, ranks whole files with BM25 (`k1=1.2`, `b=0.75`), normalizes scores with
`s/(s+1)`, and adds CJK unigram/bigram tokens. It emits the inferred consumer
shape `{query,results:[{path,score,snippets}]}`.

This deliberately diverges in three ways:

1. no persistent index or `index_not_built` prerequisite;
2. no embedding model, dense arm, or RRF fusion;
3. lexical relevance rather than semantic/cosine relevance.

The trade keeps the command offline, immediate, and compatible with the
project's zero-heavy-runtime-dependency distribution model. The generated
`ask` skill consumes `error`, `results[].path`, and relative score strength;
it remains compatible because successful OpenSpectra responses omit `error`,
keep paths within the configured spec directory, and return monotonic scores
in `(0, 1)`.
