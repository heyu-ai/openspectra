# Test plan: dependency-free search

| Case | Coverage | Expected result |
|---|---|---|
| Core ranking | Specific multi-term document vs common match | Specific document ranks first; scores are in `(0, 1)` |
| CJK phrase | Tokenize `封存變更` | Contains `封存`, `存變`, and `變更` bigrams |
| Corpus | specs, active changes, archive | All three are discoverable |
| Empty corpus | Missing/empty spec directory | Successful empty response |
| Limit zero | `--limit 0` | Successful empty response |
| JSON contract | Matching fixture with `--json` | Compact JSON, plural `snippets`, no `error` |
| Initialization | Search outside a project | Exit 1 and initialization error on stderr |
| Human empty output | No matching documents | `No results found.` |

Release verification additionally runs fmt, clippy with warnings denied,
locked release build, all tests, and a real-project CLI smoke test.
