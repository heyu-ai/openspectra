# tasks.md — add-vector-search

> [PRIORITY-REVIEW] 優先序自動推導，請確認後移除此行。
> 分階段見 design.md「分階段建議」：Phase A（可獨立交付）→ B（oracle-gated）→ C（選用）。
> 所有 search 程式碼與依賴置於 `--features search`（C1）。

## Phase 1：Setup

- [ ] T001 在 `spectra-core/Cargo.toml` 新增 `[features] search = ["dep:ort", "dep:tokenizers", "dep:tantivy", "dep:rusqlite", "dep:hf-hub"]`，各 dep 標 `optional = true` — target: `crates/spectra-core/Cargo.toml`
- [ ] T002 在 `spectra-cli/Cargo.toml` 傳遞 `search` feature 到 core；預設不啟用 — target: `crates/spectra-cli/Cargo.toml`
- [ ] T003 建立 `search/` 模組骨架與 `#[cfg(feature = "search")]` gate，`lib.rs` 條件 re-export — target: `crates/spectra-core/src/search/mod.rs`, `crates/spectra-core/src/lib.rs`

## Phase 2：Foundational（阻斷性前置依賴）

- [ ] T010 [P] 模型層：e5-small qonnx 資產解析、快取路徑、`ort` session 初始化 — target: `crates/spectra-core/src/search/model.rs`
- [ ] T011 [US3] 模型下載（`hf-hub` 或 HTTPS）：暫存 → 完整性校驗 → atomic rename（AC-003-2）— target: `crates/spectra-core/src/search/model.rs`
- [ ] T012 [P] embed API：`passage:`/`query:` 前綴 + tokenize + ort 前向 + L2-normalize → 384-d f32（C4）— target: `crates/spectra-core/src/search/model.rs`
- [ ] T013 [P] `.vector-search.db` schema（documents/embeddings/meta）rusqlite 建表 — target: `crates/spectra-core/src/search/index.rs`

## Phase 3：User Stories（P1 → P2 → P3）

### US-002：索引生命週期（P1 — 其他 US 的前提）

**Story Goal**：掃 corpus、chunk、建/重建 `.vector-search.db` + tantivy 索引。
**Test traceability**：AC-002-1~5 → IDX-EP-001, IDX-VL-001, IDX-ST-001, IDX-BVA-001, IDX-EP-002
  Verification：`cargo test --features search -p spectra-core search::index`

- [ ] T020 [P] [US2] corpus 掃描：`specs/**` + 已封存 `changes/<YYYY-MM-DD>-*/**`（AC-002-1）— target: `crates/spectra-core/src/search/index.rs`
- [ ] T021 [US2] per-heading chunking（design 決策）— target: `crates/spectra-core/src/search/index.rs`
- [ ] T022 [US2] build/rebuild：寫 documents+embeddings+meta，冪等（`corpus_hash`），移除文件不殘留（AC-002-3/4）— target: `crates/spectra-core/src/search/index.rs`
- [ ] T023 [P] [US2] tantivy BM25 index schema 與建置（sidecar 目錄）— target: `crates/spectra-core/src/search/bm25.rs`
- [ ] T024 [US2] `.gitignore` 確保含 `.vector-search.db` + tantivy sidecar（AC-002-2；與 `init` 協調）— target: `crates/spectra-core/src/search/index.rs`（或 `init.rs`）
- [ ] T025 [US2] 索引缺失時明確錯誤（AC-002-5）— target: `crates/spectra-core/src/search/mod.rs`
- [ ] T026 [P] [US2] 單元測試 IDX-* — target: `crates/spectra-core/src/search/index.rs`（`#[cfg(test)]`）

### US-001：hybrid 查詢（P1 — 核心交付）

**Story Goal**：BM25 + dense 兩路 → RRF 融合 → top-N。
**Test traceability**：AC-001-1~6 → SRCH-VL-001~003, SRCH-DT-001, SRCH-RB-001, SRCH-EP-001, SMK-001/002
  Verification：`cargo test --features search -p spectra-core search::` + `-p spectra-cli search`

- [ ] T030 [P] [US1] dense arm：query embed（`query:` 前綴）+ flat cosine scan over embeddings（AC-001-2）— target: `crates/spectra-core/src/search/mod.rs`
- [ ] T031 [P] [US1] sparse arm：tantivy BM25 查詢封裝 — target: `crates/spectra-core/src/search/bm25.rs`
- [ ] T032 [US1] RRF 融合（k=60，inferred；標記待校準）（AC-001-3）— target: `crates/spectra-core/src/search/fusion.rs`
- [ ] T033 [US1] `query()` 組裝：兩路 → 融合 → top-`limit` → `{path, score, snippet}`（AC-001-1）；空 corpus 回空集（AC-001-6）— target: `crates/spectra-core/src/search/mod.rs`
- [ ] T034 [US1] CLI `Search { query, limit, json }` 變體 + handler（薄殼，C2）— target: `crates/spectra-cli/src/main.rs`
- [ ] T035 [US1] `--json` 契約 `{query, results, count}`（AC-001-4）— target: `crates/spectra-cli/src/main.rs`
- [ ] T036 [P] [US1] 單元測試 SRCH-*（含 e5 前綴斷言 SRCH-VL-002、RRF 非單路 SRCH-DT-001、注入式零網路 SRCH-RB-001）— target: `crates/spectra-core/src/search/*.rs`
- [ ] T037 [US1] integration：`search --json` shape + 空 corpus（SRCH-VL-003, SRCH-EP-001）— target: `crates/spectra-cli/tests/search_integration.rs`

### US-003：模型取得與 feature gate（P2）

**Story Goal**：首次下載、離線可用、未含 feature 時清楚訊息。
**Test traceability**：AC-003-1~5 → MDL-EP-001, MDL-RB-001, MDL-DT-001, SMK-003
  Verification：`cargo test --features search model` + 預設 build 的 cfg 測試

- [ ] T040 [US3] feature-off 路徑：CLI 對 `search` 回清楚訊息 + 非 0（AC-003-5，非 generic unrecognized）— target: `crates/spectra-cli/src/main.rs`
- [ ] T041 [US3] 首次下載告知/opt-in（AC-003-4）— target: `crates/spectra-core/src/search/model.rs`
- [ ] T042 [P] [US3] 測試 MDL-RB-001（中斷下載）、MDL-DT-001（feature-off 訊息）；MDL-EP-001 標 `#[ignore]` — target: `crates/spectra-core/src/search/model.rs`, `crates/spectra-cli/tests/search_integration.rs`

### US-004：oracle 校準（P3 — oracle-gated，Phase B）

**Story Goal**：釘死 chunk/RRF k/相似度，對齊原版 top-N 集合。
**Test traceability**：AC-004-1~4 → SRCH-CAL-001
  Verification：`scripts/calibrate-search.py`（macOS + Spectra.app）

- [ ] T050 [US4] `scripts/calibrate-search.py`：同 corpus/query 跑 oracle 取 golden top-N，比對集合（AC-004-1/2）— target: `scripts/calibrate-search.py`
- [ ] T051 [US4] 依校準結果釘 chunk 規則 / RRF k / 度量，更新 `docs/reverse-engineering/search.md` measured 標註（AC-004-4, C5）— target: `crates/spectra-core/src/search/*.rs`, `docs/reverse-engineering/search.md`
- [ ] T052 [US4] 無 oracle 時標 issue `blocked: needs-oracle`，其餘 US 照常交付（AC-004-3）— target: roadmap/issue

## Phase 4：Polish / 發佈（Phase C，選用）

- [ ] T060 [P] CI 新增 `--features search` build+test job（ubuntu-only 起步）— target: `.github/workflows/ci.yml`
- [ ] T061 [P] 發佈變體：含 search 的 binary 為獨立 artifact，不取代預設（roadmap Phase 3 銜接）— target: `.github/workflows/release.yml`
- [ ] T062 更新 `docs/roadmap.md`：新增 semantic-search phase，連結本 change 與 `search.md` — target: `docs/roadmap.md`
- [ ] T063 更新 `README.md` 指令清單與 `CLAUDE.md`（`--features search` 驗證指令）— target: `README.md`, `CLAUDE.md`

## 全量驗證（完工前，C3）

- [ ] V1 `cargo fmt --all -- --check`
- [ ] V2 `cargo clippy --all-targets --features search -- -D warnings`
- [ ] V3 `cargo build --release --locked --features search` **且** `cargo build --release --locked`（預設無 feature 亦須綠，證明 gate 隔離）
- [ ] V4 `cargo test --all --features search` **且** `cargo test --all`（預設不含 search 測試）
