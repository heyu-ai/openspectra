# Design：add-vector-search

技術設計。反推事實見 [`docs/reverse-engineering/search.md`](../../../reverse-engineering/search.md)；
本檔決定 OpenSpectra 的實作選型與模組佈局。標「(RE)」= 反推自 binary，標「(決策)」=
本專案設計決定。

## 架構總覽

```
spectra search "<query>"  (CLI, spectra-cli)
        │
        ▼
spectra-core::search::query()
        │
   ┌────┴─────────────────────────┐
   ▼                              ▼
sparse arm                    dense arm
tantivy BM25 (RE)             e5-small ONNX (RE)
over .vector-search.db        ort + tokenizers
   │                              │
   └────────────┬─────────────────┘
                ▼
        RRF fusion (RE: reciprocal/rrf)
                ▼
        top-N {path, score, snippet}
```

索引管線（離線 / 顯式重建）：

```
corpus scan (specs/** + archived changes/**)
   → chunk (決策：per-heading，見下)
   → BM25 index (tantivy) + dense vectors (e5, passage: 前綴)
   → persist .vector-search.db
```

## 選型（Crate 決策）

| 面向 | 選擇 | 理由 |
|------|------|------|
| ONNX 推論 | `ort`（ONNX Runtime binding）(RE：binary 用 ort) | 與原版同引擎，降低 embedding 差異；CoreML EP 在 macOS，CPU EP 跨平台 |
| Tokenizer | `tokenizers`（HF）(RE：`tokenizer.json` + build path) | e5 的 tokenizer.json 直接載入，與原版一致 |
| Sparse/BM25 | `tantivy`(RE：binary 含 `tantivy`) | 與原版同 BM25 實作，ranking 特性一致 |
| 向量儲存 | `rusqlite`（SQLite）(決策，RE 僅知檔名 `.vector-search.db`) | 檔名暗示 SQLite 家族；單檔、無伺服器、易 gitignore。schema 見下 |
| 向量相似度 | flat cosine scan (決策；RE 無 `hnsw`) | corpus 數百 doc，暴力掃足夠；避免 ANN 複雜度（見 A2） |
| 模型下載 | `hf-hub` crate 或直接 HTTPS (決策) | e5-small 為公開 HF 模型，不需原版 runtime URL（RE：URL 無明碼） |

> 所有上述依賴**僅在 `--features search` 啟用**（C1）。預設 build 不引入 ONNX Runtime，
> 維持「單一 musl 靜態 binary、只需 git」的發佈保證（roadmap Phase 3）。

## 模組佈局（spectra-core）

```
crates/spectra-core/src/search/
├── mod.rs        # pub fn query(...) -> SearchResults；feature-gated
├── model.rs      # e5-small 資產取得、快取、ort session、embed(passage/query)
├── index.rs      # corpus 掃描、chunk、build/rebuild、.vector-search.db 讀寫
├── bm25.rs       # tantivy schema 與查詢封裝
└── fusion.rs     # RRF：兩路 ranked list → 融合排序
```

`crates/spectra-cli/src/main.rs`：新增 `Search { query, limit, json }` 變體與 handler，
薄殼呼叫 `spectra_core::search::query`（C2）。整個 `search` 模組與 CLI 變體置於
`#[cfg(feature = "search")]`；未啟用時 CLI 對 `search` 子指令回 AC-003-5 的明確訊息
（決策：以 stub 變體或 build-cfg 分支處理，而非讓 clap 回 generic unrecognized）。

## Entity：VectorSearchDb（`.vector-search.db`，SQLite）(決策 schema)

> RE 僅確定檔名與「gitignored、專案根目錄」。以下 schema 為本專案設計；oracle 校準
> （US-004）不要求 schema 一致，只要求查詢結果集一致，故此處可自由設計。

| Table | 欄位 | 說明 |
|-------|------|------|
| `documents` | `id INTEGER PK`, `path TEXT`, `heading TEXT NULL`, `content TEXT`, `kind TEXT` | 每個 chunk 一列；`kind ∈ spec \| archived`；`path` 相對專案根 |
| `embeddings` | `doc_id INTEGER FK`, `vector BLOB` | 384 個 f32（little-endian）= 1536 bytes；`passage:` 前綴後的 e5 輸出 |
| `meta` | `key TEXT PK`, `value TEXT` | `model_name`、`dim=384`、`built_at`、`corpus_hash`（重建冪等/失效判斷用） |

- **BM25**：由 `tantivy` 自管其索引檔（放 `.vector-search.db` 旁的 sidecar 目錄，或
  同納一 SQLite blob——index.rs 決定；決策傾向 sidecar 目錄，tantivy 原生檔案格式）。
- `.gitignore`：本 change 確保 `.vector-search.db`（及 tantivy sidecar）被忽略；若
  `spectra init` 尚未涵蓋則補上 entry（AC-002-2）。

## Chunking（決策，inferred from RE `chunk` string）

RE 只知有 chunking，未知規則。本專案採 **per-heading chunk**：以 Markdown 標題
（`#`/`##`/`###`）切段，每段為一 document。理由：spec.md 的 Gherkin scenario、AC 天然
以標題分節，per-heading 讓命中片段可直接對應到一個 scenario/AC，`snippet` 與 `path`
（可含 `#heading`）對讀者最有用。oracle 校準時若發現原版用固定 token window，於 index.rs
調整並更新 `search.md`（C5）。

## e5 embedding 契約（RE，正確性不變量 C4）

```
index 時： embed("passage: " + chunk_text)
query 時： embed("query: "   + query_text)
```

- 兩個前綴不可省、不可互換（e5 非對稱設計）。以測試 `e5-prefix-applied` 守住
  （SRCH-VL-002）：斷言送進 tokenizer 的字串帶正確前綴。
- 輸出 384-d f32，L2-normalize 後存入 / 比對（cosine == normalized dot product，RE
  strings `cosine`/`dotproduct`/`normalize`）。

## RRF 融合（RE：`reciprocal`/`rrf`）

```
score(doc) = Σ_arm  1 / (k + rank_arm(doc))
```

兩路（BM25、dense）各自產 ranked list，對每個 doc 取其在各 list 的名次代入 RRF，加總為
最終分數，降冪取 top-`limit`。`k` 常數 RE 未知（inferred）：**先用文獻慣用 `k=60`**，
標為 inferred，待 US-004 校準釘死並更新 `search.md`（AC-004-4）。

## 錯誤與離線行為

| 情境 | 行為 |
|------|------|
| 無 `.vector-search.db` | AC-002-5：明確錯誤「index not found, run rebuild」（決策：不自動建，避免大 corpus 下 `search` 意外觸發長時間索引） |
| 無模型且離線 | 明確錯誤，指向模型下載步驟 |
| 空 corpus | AC-001-6：`results: []`, exit 0 |
| 未含 `--features search` | AC-003-5：清楚訊息 + 非 0 exit |
| 下載中斷 | AC-003-2：不留半殘可用狀態（下載到暫存路徑，完整校驗後 atomic rename） |

## 發佈影響（C1，roadmap Phase 3 銜接）

- 預設 `cargo build --release`（無 feature）：產物與現況相同，drift-only，musl 靜態可行。
- `--features search`：連結 ONNX Runtime（動態或靜態視 `ort` 設定），產物較大、平台相依性
  較高。發佈為**獨立變體**（如 `spectra-search-x86_64-linux`），不取代預設 binary。
- CI：新增一個 `--features search` 的 build+test job（可先 ubuntu-only）；模型下載測試
  （MDL-EP-001）為 `#[ignore]` + opt-in 網路 job，不進預設 CI gate。

## 衝突偵測

- 既有 `spectra-core` 模組（drift/anchors/git/tasks/...）皆無 `search` 命名；新增
  `search` 子模組無命名衝突。
- CLI 既有子指令（init/drift/validate/list/show/park/unpark/new/task/archive）無 `search`；
  新增不衝突（且對齊 RE measured 的原版指令名）。
- baseline：`docs/openspec/specs/` 目前無 `vector-search` capability，屬新增。

## 分階段建議（見 tasks.md）

1. **Phase A（可獨立交付）**：US-001~003，`--features search`，flat cosine + tantivy + RRF(k=60)，
   per-heading chunk。產出「合理 hybrid 搜尋」，不依賴 oracle。
2. **Phase B（oracle-gated）**：US-004 校準，釘 chunk/k/度量，更新 `search.md`。
3. **Phase C（選用）**：發佈變體、CI 網路 job、（若 corpus 變大）ANN 評估。
