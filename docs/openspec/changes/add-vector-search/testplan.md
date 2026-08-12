# Test plan: dependency-free search

Techniques: EP = Equivalence Partitioning, BVA = Boundary Value Analysis,
DT = Decision Table, ST = State Transition, RB = Risk-Based.

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result | Traces to |
|---|---|---|---|---|---|---|---|---|
| TC-01 | Specific multi-term doc outranks a common single-term doc | EP | Med | Two `.md` docs under `spec_dir` | `search("token rotation", 10)` | `auth/spec.md`="token token rotation policy"; `other/spec.md`="general token notes" | Specific doc ranks first | `ranks_specific_document_above_common_match_and_normalizes_score` |
| TC-02 | Raw BM25 scores normalize to `(0, 1)` | BVA | Med | Matching corpus | Inspect result scores | Same as TC-01 | Every score `> 0.0` and `< 1.0` | `ranks_specific_document_above_common_match_and_normalizes_score` |
| TC-03 | Chinese CJK query tokenizes to unigrams + bigrams | EP | Med | n/a (unit) | `tokenize("封存變更")` | `封存變更` | Contains `封存`, `存變`, `變更` | `cjk_bigrams_distinguish_phrases` |
| TC-04 | Korean Hangul splits into unigrams + bigrams | EP | High | n/a (unit) | `tokenize("검색")` | `검색` | Contains `검`, `색`, `검색` | `hangul_tokenizes_into_unigrams_and_bigrams` |
| TC-05 | Japanese/Korean short query matches longer unspaced phrase | EP | High | Corpus with `검색기능`, `ひらがな` docs | `search("검색")`, `search("ひらがな")` | Hangul + kana phrases | Each matches its document | `kana_and_hangul_queries_match_unspaced_phrases` |
| TC-06 | Corpus spans specs, active changes, and archive | EP | Med | Three docs across the three areas | `search("unique", 10)` | one doc in each area | All three discoverable | `scans_active_and_archived_changes_as_well_as_specs` |
| TC-07 | Empty / missing spec directory | RB | Med | Missing `spec_dir` | `search("anything", 10)` | none | Successful empty result | `zero_limit_and_empty_corpus_return_no_results` |
| TC-08 | `--limit 0` short-circuits | BVA | Med | Any corpus | `search("anything", 0)` | limit=0 | Successful empty result | `zero_limit_and_empty_corpus_return_no_results` |
| TC-09 | `--limit N` caps below match count | BVA | High | Three docs all match `token` | `search("token", 1)` | 3 matching docs | Exactly 1 result returned | `limit_caps_results_below_match_count` |
| TC-10 | Equal-scoring docs order deterministically by path | ST | High | Two identical-content docs | `search("token rotation", 10)` | `a.md`, `b.md` identical | Equal scores; `a.md` before `b.md` | `equal_scoring_documents_order_by_path` |
| TC-11 | Snippet survives Unicode lowercase expansion before a match | BVA | High | Doc with expanding chars before needle | `snippet(content, "needle")` | `İ`×100 + `needle` | No panic; snippet contains `needle` | `snippet_handles_lowercase_expansion_before_a_match` |
| TC-12 | `spec_dir` lexical escape (`..`, absolute) rejected | RB | Critical | Config with escaping `spec_dir` | `search("secret", 10)` | `../outside`, `/tmp/outside` | Errors "spec_dir must be a relative path within the project root" | `rejects_spec_directories_that_escape_the_project_root` |
| TC-13 | Symlinked `spec_dir` cannot surface external files | RB | Critical | `spec_dir` is a symlink to a dir outside root | `search("topsecret", 10)` | `openspec -> <outside>/secret.md` | Empty result (no leak) | `symlinked_spec_dir_cannot_surface_files_outside_the_root` |
| TC-14 | Symlinked `.md` inside `spec_dir` cannot surface external files | RB | Critical | Symlinked `.md` resolving outside root | `search("topsecret", 10)` | `specs/leak.md -> <outside>/secret.md` | Empty result (no leak) | `symlinked_markdown_inside_spec_dir_is_not_surfaced` |
| TC-15 | JSON `--json` consumer contract shape | DT | High | Matching fixture | run CLI `search --json` | fixture doc | Compact JSON, plural `snippets`, no `error` | `search_json_has_consumer_contract_and_honors_limit` (tests/search_integration.rs) |
| TC-16 | Text mode empty output matches oracle message | EP | Med | No matching documents | run CLI `search` | non-matching query | `No results found.` | `text_mode_matches_observable_oracle_messages` (tests/search_integration.rs) |
| TC-17 | Search outside an initialized project | RB | Med | Uninitialized cwd | run CLI `search` | none | Exit 1 + `Not initialized` on stderr | `search_requires_init_and_empty_project_is_successful` (tests/search_integration.rs) |

Release verification additionally runs fmt, clippy with warnings denied,
locked release build, all tests, and a real-project CLI smoke test.
