# Proposal：add-vector-search

> 版本：v1.0 | 日期：2026-07-19 | 狀態：Draft
> 來源：使用者要求「反推 `spectra search`（GUI 向量搜尋）並給出開發計畫」
> 依據：`docs/reverse-engineering/search.md`（本 PR 同批新增的 RE write-up）

## Why

閉源 `spectra` v2.3.1 有一個 `spectra search <query>` 指令，對專案的 spec 與已封存
變更做**語意檢索**（GUI 呈現為「向量搜尋 / 向量模型 / 語意搜尋索引」）。OpenSpectra
目前**完全沒有**這塊——`drift.md:13-15` 當初刻意把它劃出 port 範圍，現行 roadmap
（Phase 1–5）也不含它。

痛點：openspec 專案的規格散落在 `changes/<name>/{proposal,design,tasks}.md` 與
`specs/<cap>/spec.md`，量一大就難以用關鍵字找到「哪份 spec 講過這件事」。原版靠
BM25 + 向量 hybrid 檢索解決；OpenSpectra 使用者拿不到這個能力，等於少了原版一個核心
指令。

這份 proposal 的目的是**評估可行性並定義開發計畫**，不是一次做完。它明確承認這是全
repo 目前**最重的一塊**（見 Out of Scope 與硬性限制），並建議以 feature-gated、分階段
方式落地。

## What Changes

新增一條 `spectra search` 指令與其底層 hybrid 檢索引擎（大尺度 / Epic）：

1. **`spectra-core::search` 模組**：hybrid 檢索核心——BM25（`tantivy`）+ dense 向量
   （`intfloat/multilingual-e5-small` quantized ONNX，經 `ort` + `tokenizers` 本機推論）
   以 RRF 融合。**狀態：未實作，本 proposal 提出**
2. **索引生命週期**：掃描 corpus（specs + archived changes）→ chunk → 建 `.vector-search.db`
   索引；支援重建與增量更新。**狀態：未實作**
3. **模型取得**：首次使用時從 Hugging Face 下載 e5-small qonnx 資產並快取；離線後純本機
   運作。**狀態：未實作**
4. **CLI `search` 子指令**：`spectra search <QUERY> --limit <N> --json`，介面對齊原版
   （見 `search.md` 的 measured CLI surface）。**狀態：未實作**
5. **build feature flag**：整個能力放在 `--features search` 之後，讓「純 drift、單一 musl
   靜態 binary」的預設發佈路徑（roadmap Phase 3）不受 ONNX Runtime 依賴影響。**狀態：未實作**

## Step 1a — 四元素萃取

| 元素 | 內容 |
|------|------|
| **Actors** | (1) 在 openspec 專案下找規格的開發者；(2) CI/腳本以 `--json` 取結構化結果；(3) 離線環境使用者；(4) 校準操作者（macOS + Spectra.app oracle） |
| **Actions** | 建立/更新語意索引、下載 embedding 模型、下 query 取 top-N 相關文件、以 `--json` 輸出、（校準）比對原版 ranking |
| **Data** | corpus = `<spec_dir>/specs/**` + 已封存 `changes/<YYYY-MM-DD>-*/**`；`.vector-search.db`（BM25 + 向量索引）；e5-small qonnx 模型資產；query 字串；結果清單 `{path, score, snippet}` |
| **Constraints** | e5 前綴（`query:` / `passage:`）不可省；查詢時零網路；模型/ONNX Runtime 依賴不得污染預設 drift-only 發佈；無 oracle 時只能做「合理 hybrid」不保證與 v2.3.1 逐名次一致；`.vector-search.db` 必須 gitignore |

## Step 1 — User Stories

### US-001：開發者用自然語言查詢專案規格（capability: `vector-search`）

**Persona**：在中大型 openspec 專案工作的開發者，記得「某份 spec 討論過 archive 的
delta 合併順序」但不記得在哪個 change / capability，關鍵字 grep 命中太多。
**Action**：執行 `spectra search "archive delta 合併順序" --limit 5`。
**Outcome**：取得最相關的前 5 份 spec 片段（含路徑、分數、片段），能直接跳到來源。

**Acceptance Criteria**：

- AC-001-1：`spectra search <QUERY>` 回傳依相關度排序的結果清單，預設 `--limit 10`
- AC-001-2：dense 檢索對 index 文件加 `passage:` 前綴、對 query 加 `query:` 前綴（e5 慣例）；缺前綴的實作視為錯誤（相關度崩壞）
- AC-001-3：最終排序為 BM25 與 dense 兩路的 **RRF 融合**，非單路結果
- AC-001-4：`--json` 輸出穩定 shape `{query, results: [{path, score, snippet}], count}`（欄位名以 oracle 捕獲後校準；捕獲前以此為暫定契約並在 `search.md` 標註）
- AC-001-5：查詢過程零網路請求（模型與索引皆已就緒時）
- AC-001-6：corpus 為空（無任何 spec/archived change）時，回傳空結果集且 exit 0，不 panic、不報錯

### US-002：索引隨規格演進而建立與更新（capability: `search-index-lifecycle`）

**Persona**：同一開發者，改了幾份 spec、封存了一個 change 之後，希望搜尋結果反映最新內容。
**Action**：執行索引重建（CLI 觸發方式見 design.md；至少提供顯式重建）。
**Outcome**：`.vector-search.db` 反映當前 corpus，過期/已刪除文件不再出現在結果中。

**Acceptance Criteria**：

- AC-002-1：索引涵蓋 `<spec_dir>/specs/**` 與已封存 `changes/<YYYY-MM-DD>-*/**`（對齊 GUI「規格與已封存變更」語意）
- AC-002-2：`.vector-search.db` 建立於專案根目錄，且被 `.gitignore`（若 `spectra init` 尚未加入 `.vector-search.db` entry，本 change 補上）
- AC-002-3：重建為冪等——同一 corpus 重建兩次，查詢結果集一致
- AC-002-4：已從 corpus 移除的文件，重建後不再出現在任何 query 結果
- AC-002-5：索引缺失時執行 `search`，回明確錯誤或自動建立（擇一，design.md 定案），不得靜默回空集混淆「無索引」與「無命中」

### US-003：首次使用自動取得模型、之後離線可用（capability: `search-model-provisioning`）

**Persona**：初次跑 `spectra search` 的使用者，本機尚無 embedding 模型。
**Action**：執行 `spectra search`，或顯式的模型下載動作。
**Outcome**：自動從 Hugging Face 取得 `multilingual-e5-small` qonnx 資產並快取；之後離線可用。

**Acceptance Criteria**：

- AC-003-1：模型不存在時，從公開 HF repo 下載 qonnx 資產（`model_quantized.onnx` + tokenizer/config 檔）至穩定快取路徑
- AC-003-2：下載完整性可驗證（大小/雜湊），半途失敗不留下半殘可用狀態
- AC-003-3：模型就緒後，查詢完全離線（AC-001-5 的前提）
- AC-003-4：下載為 opt-in 或首次明確告知——不可在使用者未預期時默默拉 ~100MB+
- AC-003-5：`--features search` 未編入時，`spectra search` 給清楚的「此 binary 未含 search，請用含 search 的發佈版」訊息，而非 unknown subcommand

### US-004：與 oracle ranking 對齊（capability: `search-oracle-calibration`，oracle-gated）

**Persona**：校準操作者，在 macOS + Spectra.app 環境驗證 OpenSpectra 的 `search` 與原版一致。
**Action**：對同一 corpus/query 集，跑原版與 OpenSpectra，比對 top-N。
**Outcome**：釘死可校準的常數（chunking、RRF k、前綴、相似度度量），差異記入 `search.md`。

**Acceptance Criteria**：

- AC-004-1：提供 `scripts/calibrate-search.py`（仿 `calibrate-*.py`）：同 corpus/query 跑 oracle 取 golden top-N，比對 OpenSpectra
- AC-004-2：校準判準為「同 query 的 top-N **文件集合**一致」（順序一致為加分項，非硬性——量化雜訊可致名次微動）
- AC-004-3：無 oracle 時本 US 標 `blocked: needs-oracle`，其餘 US 仍可獨立交付「合理 hybrid 搜尋」
- AC-004-4：任何為對齊 oracle 而定的常數（RRF k、chunk 大小）在 `search.md` 標明「measured vs inferred」

## Step 4 — 假設與約束

### 假設

| # | 假設內容 | 若不成立的影響 |
|---|----------|----------------|
| A1 | 反推的架構正確：hybrid = BM25(`tantivy`) + e5-small dense，RRF 融合（`search.md` measured strings） | 若原版實際為單路或不同融合，ranking 對不上 oracle；US-004 校準會抓出，需修 design.md |
| A2 | dense 索引為 flat cosine scan（未見 `hnsw` 字串，corpus 僅數百 doc） | 若原版用 ANN，效能特性不同但結果集應相近；大 corpus 時 OpenSpectra 需再評估 ANN |
| A3 | chunking 規則、RRF k、相似度度量細節未經 oracle 觀測 | 影響逐名次一致性；捕獲前只能保證「合理 hybrid」，AC-004 收尾 |
| A4 | e5-small qonnx 為公開 HF 模型、可直接下載，無需破解原版下載 URL（runtime 組出、binary 無明碼） | 若 HF 資產與原版微調版不同，embedding 會有差異；以 US-004 比對確認 |
| A5 | `spectra search --json` 的實際 shape 未捕獲（probe 機無模型） | AC-001-4 的契約為暫定；oracle 捕獲後可能需調整欄位名 |

### 硬性限制

| # | 限制 | 來源 |
|---|------|------|
| C1 | 整個能力置於 `--features search` 之後；預設 build 與現行 drift-only、單一 musl 靜態 binary 發佈路徑不受影響 | roadmap Phase 3 發佈策略、CLAUDE.md「CLI 薄殼、核心在 spectra-core」 |
| C2 | 新邏輯放 `spectra-core::search`，CLI crate 僅接線；unit test 住模組內 `#[cfg(test)]`，integration test 住 `crates/spectra-cli/tests/` | CLAUDE.md workspace 慣例 |
| C3 | 完工前四道全量驗證全綠：`cargo fmt --all -- --check`、`cargo clippy --all-targets --features search -- -D warnings`、`cargo build --release --locked --features search`、`cargo test --all --features search` | CLAUDE.md 硬門檻（加 `--features search`） |
| C4 | e5 前綴（`query:`/`passage:`）與「查詢零網路」為正確性不變量，須有測試守住 | `search.md` measured；e5 模型契約 |
| C5 | RE'd 常數（模型名、維度、融合法）任何變動須同步更新 `docs/reverse-engineering/search.md` | CLAUDE.md「RE 常數變動同步 write-up」 |
| C6 | Commit 拆分、explicit refspec push、開 draft PR、勿 merge | repo git 慣例 |

### Out of Scope

| 功能 | 排除原因 | 未來考量 |
|------|----------|----------|
| GUI／桌面 App 模式（「重建索引 / 刪除索引」按鈕、模型下載 UI） | OpenSpectra 是 CLI；原版 GUI 與 CLI 同 binary，但重寫 GUI 非本專案目標 | 若未來做 TUI/GUI 再議 |
| 逐名次 byte-identical 對齊 oracle | 量化/chunking 雜訊使其不切實際；判準改為 top-N 集合一致 | US-004 持續校準，量測配結構論證 |
| ANN（HNSW）向量索引 | 未見於 binary；corpus 數百 doc，flat scan 已足 | corpus 顯著變大時評估 |
| 索引即時 file-watch 自動更新 | 原版靠顯式重建；自動 watch 是額外複雜度 | Phase 化後評估 |
| 把 search 納入預設 binary | ONNX Runtime + 模型下載破壞「單一靜態 binary、只需 git」的發佈保證 | 維持 feature-gated，另出含 search 的發佈變體 |

## Step 5 — 完工標準

### Done 定義

此 change 視為「完成」的條件（US-001~003 為第一階段；US-004 oracle-gated）：

- [ ] US-001~003 的 AC 均已實作（US-004 依 oracle 可用性，無則標 `blocked: needs-oracle`）
- [ ] testplan.md 所有 TC 均有對應 Rust 測試或標注為手動/CI 驗證項
- [ ] 冒煙測試（SMK-001~003）通過
- [ ] 四道全量驗證（C3，含 `--features search`）全綠；且**預設 build（無 feature）**仍全綠，證明 feature gate 有效隔離
- [ ] `docs/reverse-engineering/search.md` 的 measured/inferred 標註與實作一致（C5）
- [ ] 程式碼已 push、draft PR 已開（C6）

### 冒煙測試情境

#### Scenario: smk-search-happy -- SMK-001 有索引時查詢命中

**GIVEN** 一個已 init、含數份 spec 且已建立 `.vector-search.db`、模型已就緒的專案
**WHEN** 執行 `spectra search "<與某份 spec 高度相關的字句>" --limit 3 --json`
**THEN** 系統 MUST 輸出合法 JSON，`results` 非空，且 top-1 的 `path` MUST 指向該 spec；查詢過程 MUST NOT 發出網路請求

#### Scenario: smk-search-empty-corpus -- SMK-002 空 corpus 不崩

**GIVEN** 一個已 init 但無任何 spec/archived change 的專案，模型已就緒
**WHEN** 執行 `spectra search "anything" --json`
**THEN** 系統 MUST exit 0 並輸出 `results: []`、`count: 0`，MUST NOT panic

#### Scenario: smk-search-feature-off -- SMK-003 未含 search 的 binary 給清楚訊息

**GIVEN** 以預設（無 `--features search`）build 出來的 binary
**WHEN** 執行 `spectra search "x"`
**THEN** 系統 MUST 回清楚的「此 binary 未含 search 能力」訊息並以非 0 退出，MUST NOT 顯示 generic「unrecognized subcommand」

### Traceability Matrix

| US | Gherkin Scenario slug | TC-ID | Rust 測試（規劃） |
|----|----------------------|-------|------------------|
| US-001 | `search-ranked-results` | SRCH-VL-001 | `search::tests::query_returns_ranked_results` |
| US-001 | `e5-prefix-applied` | SRCH-VL-002 | `search::tests::index_uses_passage_query_uses_query_prefix` |
| US-001 | `rrf-fusion-not-single-arm` | SRCH-DT-001 | `search::tests::final_order_is_rrf_of_bm25_and_dense` |
| US-001 | `json-shape` | SRCH-VL-003 | `tests::search_json_shape_matches_contract`（spectra-cli） |
| US-001 | `query-offline` | SRCH-RB-001 | `search::tests::query_makes_no_network_calls`（注入式 fetch 斷言） |
| US-001 | `empty-corpus-empty-result` | SRCH-EP-001 | `search::tests::empty_corpus_returns_empty_not_error` |
| US-002 | `index-covers-specs-and-archived` | IDX-EP-001 | `search::index::tests::indexes_specs_and_archived_changes` |
| US-002 | `db-gitignored` | IDX-VL-001 | `search::index::tests::db_path_is_project_root_and_gitignored` |
| US-002 | `reindex-idempotent` | IDX-ST-001 | `search::index::tests::rebuild_is_idempotent` |
| US-002 | `removed-doc-drops-out` | IDX-BVA-001 | `search::index::tests::removed_document_absent_after_rebuild` |
| US-002 | `missing-index-explicit` | IDX-EP-002 | `search::index::tests::missing_index_is_explicit_not_silent_empty` |
| US-003 | `model-download-on-first-use` | MDL-EP-001 | integration（網路，`#[ignore]` 預設；CI opt-in job） |
| US-003 | `download-integrity` | MDL-RB-001 | `search::model::tests::partial_download_leaves_no_usable_state` |
| US-003 | `feature-off-message` | MDL-DT-001 | `tests::search_without_feature_gives_clear_message`（build-cfg 驗證，SMK-003） |
| US-004 | `topn-set-parity` | SRCH-CAL-001 | `scripts/calibrate-search.py`（oracle-gated，手動/CI-opt-in） |
