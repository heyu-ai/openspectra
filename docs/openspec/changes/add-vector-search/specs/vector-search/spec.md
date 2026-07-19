# Specs：add-vector-search

Gherkin scenarios（RFC 2119）for the `spectra search` hybrid retrieval
feature. Slug 規則：kebab-case、唯一、顯式命名。行為常數（RRF k、chunk 大小）
在 oracle 校準前為 inferred，見 `docs/reverse-engineering/search.md`。

## US-001：開發者用自然語言查詢專案規格

### AC-001-1 / AC-001-3

#### Scenario: search-ranked-results -- 依相關度回傳前 N 份

**GIVEN** 一個已 init、已建索引、模型就緒的專案，`specs/` 下有多份 spec
**WHEN** 開發者執行 `spectra search "<query>" --limit 5`
**THEN** 系統 MUST 回傳至多 5 筆結果，依融合分數由高到低排序
  AND 每筆 MUST 含來源 `path` 與可讀 `snippet`
  AND 排序 MUST 為 BM25 與 dense 兩路的 RRF 融合結果，MUST NOT 只取單一路

### AC-001-2

#### Scenario: e5-prefix-applied -- e5 前綴正確套用

**GIVEN** e5-small 模型就緒
**WHEN** 系統為 index 文件與 query 產生 embedding
**THEN** index 文件 MUST 以 `passage: ` 前綴後再 embed
  AND query MUST 以 `query: ` 前綴後再 embed
  AND MUST NOT 對兩者省略前綴或交換前綴（e5 非對稱契約，省略致相關度崩壞）

### AC-001-4

#### Scenario: json-shape -- --json 穩定契約

**GIVEN** 已建索引、模型就緒的專案
**WHEN** 開發者執行 `spectra search "<query>" --json`
**THEN** 系統 MUST 輸出合法 JSON，頂層含 `query`、`results`、`count`
  AND `results[]` 每筆 MUST 含 `path`、`score`、`snippet`
  AND `count` MUST 等於 `results` 長度

### AC-001-5

#### Scenario: query-offline -- 查詢零網路

**GIVEN** 模型與索引皆已就緒且無網路連線
**WHEN** 開發者執行 `spectra search "<query>"`
**THEN** 系統 MUST 正常回傳結果
  AND MUST NOT 發出任何網路請求

### AC-001-6

#### Scenario: empty-corpus-empty-result -- 空 corpus 不崩

**GIVEN** 一個已 init 但 `specs/` 與已封存 changes 皆為空、模型就緒的專案
**WHEN** 開發者執行 `spectra search "anything" --json`
**THEN** 系統 MUST exit 0
  AND MUST 輸出 `results: []` 與 `count: 0`
  AND MUST NOT panic 或回非 0

## US-002：索引隨規格演進而建立與更新

### AC-002-1 / AC-002-2

#### Scenario: index-covers-specs-and-archived -- 索引涵蓋範圍與位置

**GIVEN** 一個含 `specs/<cap>/spec.md` 與已封存 `changes/2026-01-01-foo/` 的專案
**WHEN** 建立索引
**THEN** 索引 corpus MUST 同時涵蓋 active specs 與已封存 changes 的文件
  AND `.vector-search.db` MUST 建於專案根目錄
  AND `.vector-search.db` MUST 被 `.gitignore` 覆蓋

### AC-002-3

#### Scenario: reindex-idempotent -- 重建冪等

**GIVEN** 一個 corpus 不變的專案
**WHEN** 連續重建索引兩次
**THEN** 同一 query 在兩次之後的結果集 MUST 一致

### AC-002-4

#### Scenario: removed-doc-drops-out -- 移除的文件不再命中

**GIVEN** 一個已索引、某 query 原本命中文件 D 的專案
**WHEN** 從 corpus 刪除 D 後重建索引
**THEN** 該 query 的結果 MUST NOT 再包含 D

### AC-002-5

#### Scenario: missing-index-explicit -- 無索引不靜默回空

**GIVEN** 一個模型就緒但尚無 `.vector-search.db` 的專案
**WHEN** 開發者執行 `spectra search "x"`
**THEN** 系統 MUST 明確處理（回「索引不存在」錯誤，或自動建立後查詢——design.md 定案其一）
  AND MUST NOT 靜默回空結果集而讓「無索引」與「無命中」無法區分

## US-003：首次使用自動取得模型、之後離線可用

### AC-003-1 / AC-003-4

#### Scenario: model-download-on-first-use -- 首次取得模型

**GIVEN** 本機尚無 e5-small qonnx 模型
**WHEN** 使用者首次觸發需要模型的操作
**THEN** 系統 MUST 從公開 HF repo 取得 qonnx 資產至穩定快取路徑
  AND 下載前 MUST 明確告知（或為 opt-in），MUST NOT 未預期地默默下載 ~100MB+

### AC-003-2

#### Scenario: download-integrity -- 下載完整性

**GIVEN** 一個下載中途被中斷的情境
**WHEN** 系統偵測到不完整資產
**THEN** 系統 MUST NOT 把半殘資產當成可用模型
  AND 下次觸發 MUST 重新取得或明確報錯

### AC-003-5

#### Scenario: feature-off-message -- 未含 search 的 binary

**GIVEN** 以預設（無 `--features search`）build 的 binary
**WHEN** 使用者執行 `spectra search "x"`
**THEN** 系統 MUST 回清楚的「此 binary 未含 search 能力」訊息
  AND MUST 以非 0 退出
  AND MUST NOT 顯示 generic 的 unrecognized subcommand 錯誤

## US-004：與 oracle ranking 對齊（oracle-gated）

### AC-004-2

#### Scenario: topn-set-parity -- top-N 文件集合一致

**GIVEN** 同一 corpus 與 query 集，於 macOS 上有可用的 Spectra.app oracle
**WHEN** 對每個 query 分別跑原版 `spectra search` 與 OpenSpectra
**THEN** 每個 query 的 top-N **文件集合** MUST 一致
  AND 順序一致為加分項，MUST NOT 因名次微動即判定失敗（量化雜訊容許）
