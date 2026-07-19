# Test Plan：add-vector-search

TC 表格（Rust 慣例：unit 住模組 `#[cfg(test)]`，integration 住
`crates/spectra-cli/tests/`）。技法縮寫：EP=Equivalence Partitioning、
BVA=Boundary Value、DT=Decision Table、ST=State Transition、RB=Risk-Based、
VL=Validation。TC-ID 前綴：`SRCH`（查詢）、`IDX`（索引）、`MDL`（模型）、`SMK`（冒煙）。

## TC 表

| TC-ID | Test Purpose | Technique | Risk | Precondition | 主要步驟 | Expected Result |
|-------|--------------|-----------|------|--------------|----------|-----------------|
| SRCH-VL-001 | query 回排序結果 | VL | 中 | 已建索引、模型就緒、多份 spec | `search "<q>" --limit 5` | ≤5 筆，分數降冪，各含 path+snippet |
| SRCH-VL-002 | e5 前綴正確 | VL | **高** | 模型就緒 | 攔截送入 tokenizer 的字串 | index 帶 `passage: `、query 帶 `query: `；無省略/互換 |
| SRCH-DT-001 | 排序為 RRF 非單路 | DT | 高 | 造一組 BM25 與 dense 名次不同的 doc | 比對最終序 vs 兩路各自序 | 最終序 == RRF(兩路)，≠ 任一單路序 |
| SRCH-VL-003 | --json 契約 | VL | 中 | 已建索引 | `search "<q>" --json` | 合法 JSON，含 query/results/count；results[] 含 path/score/snippet；count==len |
| SRCH-RB-001 | 查詢零網路 | RB | 高 | 模型+索引就緒，注入式 http client | 查詢並斷言 client 未被呼叫 | 零網路請求 |
| SRCH-EP-001 | 空 corpus 不崩 | EP | 中 | 已 init、corpus 空、模型就緒 | `search "x" --json` | exit 0、`results:[]`、`count:0`、不 panic |
| IDX-EP-001 | 索引涵蓋 specs+archived | EP | 中 | 含 active spec + 已封存 change | 建索引後查兩者各自內容 | 兩類文件皆可命中 |
| IDX-VL-001 | DB 位置與 gitignore | VL | 中 | 已 init 專案 | 建索引 | `.vector-search.db` 在根目錄且被 `.gitignore` 覆蓋 |
| IDX-ST-001 | 重建冪等 | ST | 中 | corpus 不變 | 連建兩次、同 query 比對 | 兩次結果集一致 |
| IDX-BVA-001 | 移除文件後不命中 | BVA | 中 | query 原命中 D | 刪 D、重建、再查 | 結果不含 D |
| IDX-EP-002 | 無索引不靜默回空 | EP | 高 | 模型就緒、無 DB | `search "x"` | 明確錯誤或自動建（design 定案），非靜默空集 |
| MDL-EP-001 | 首次下載模型 | EP | 中 | 本機無模型（網路） | 觸發需模型操作 | 從 HF 取得 qonnx 資產至快取；下載前告知/opt-in。`#[ignore]`+opt-in job |
| MDL-RB-001 | 下載完整性 | RB | 高 | 模擬中斷下載 | 偵測不完整資產 | 不當可用；重取或報錯（暫存+atomic rename） |
| MDL-DT-001 | 未含 feature 訊息 | DT | 中 | 預設 build（無 feature） | `search "x"` | 清楚「未含 search」訊息 + 非 0，非 generic unrecognized |
| SMK-001 | 有索引查詢命中（e2e） | RB | 高 | 已 init+索引+模型 | `search "<相關句>" --limit 3 --json` | results 非空，top-1 path 指向該 spec，零網路 |
| SMK-002 | 空 corpus e2e | RB | 中 | 已 init、corpus 空、模型就緒 | `search "anything" --json` | exit 0、空集、不 panic |
| SMK-003 | feature-off e2e | RB | 中 | 預設 build binary | `search "x"` | 清楚訊息 + 非 0 exit |
| SRCH-CAL-001 | top-N 集合對齊 oracle | RB | — | macOS + Spectra.app、同 corpus/query | `calibrate-search.py` 跑雙方比對 | 每 query top-N 文件集合一致（順序加分）。oracle-gated |

## Coverage Analysis

| Scenario slug | 狀態 | 對應 TC |
|---------------|------|---------|
| `search-ranked-results` | ✓ covered | SRCH-VL-001, SMK-001 |
| `e5-prefix-applied` | ✓ covered | SRCH-VL-002 |
| `rrf-fusion-not-single-arm` | ✓ covered | SRCH-DT-001 |
| `json-shape` | ✓ covered | SRCH-VL-003 |
| `query-offline` | ✓ covered | SRCH-RB-001, SMK-001 |
| `empty-corpus-empty-result` | ✓ covered | SRCH-EP-001, SMK-002 |
| `index-covers-specs-and-archived` | ✓ covered | IDX-EP-001 |
| `db-gitignored` | ✓ covered | IDX-VL-001 |
| `reindex-idempotent` | ✓ covered | IDX-ST-001 |
| `removed-doc-drops-out` | ✓ covered | IDX-BVA-001 |
| `missing-index-explicit` | ✓ covered | IDX-EP-002 |
| `model-download-on-first-use` | △ partial | MDL-EP-001（網路依賴，預設 CI 不跑） |
| `download-integrity` | ✓ covered | MDL-RB-001 |
| `feature-off-message` | ✓ covered | MDL-DT-001, SMK-003 |
| `topn-set-parity` | ✗ missing（oracle-gated） | SRCH-CAL-001（需 macOS+oracle） |

**Missing/Partial 說明**：`topn-set-parity` 在無 oracle 時無法驗證，US-004 標
`blocked: needs-oracle`。`model-download-on-first-use` 依賴網路，以 `#[ignore]` +
opt-in job 覆蓋，不進預設 gate（避免 CI 對外部 HF 的脆弱依賴）。
