# Change Analyze

<!-- capability: change-analyze -->

## US-104：agent 分析 artifacts 一致性與缺口

**Persona**：sdd plugin 驅動的 agent，artifacts 產出後進入品質迴圈
（`spectra analyze <name> --json`，只看 Critical + Warning，最多 2 次）。
**Action**：執行 `spectra analyze --json`。
**Outcome**：取得 4 維度 findings 清單，據以修正 artifacts。

### Scenarios

#### Scenario: analyze-insufficient -- artifacts 不足時全維度 skip

**GIVEN** 一個尚無任何 artifact 檔的 change
**WHEN** 使用者執行 `spectra analyze --json`
**THEN** 4 個 dimension 的 `status` MUST 為 `Skipped (insufficient artifacts)`
  AND `findings` MUST 為空陣列
  AND `artifacts_missing` MUST 列出 `["proposal","specs","design","tasks"]`
  AND exit code MUST 為 0

#### Scenario: analyze-findings -- 有 findings 時的報告結構

**GIVEN** change 內 spec 有一條 requirement 且 tasks.md 無對應 task
**WHEN** 使用者執行 `spectra analyze --json`
**THEN** `findings[]` MUST 含 `id`（`COV-1` 形式）、`dimension`、`severity`、
      `location`、`summary`、`recommendation`、
      `summary_msg{key,params}`、`recommendation_msg{key,params}`
  AND 該 finding 的 key MUST 為 `covMissingTask.*`、severity MUST 為 `Warning`
  AND 對應 dimension 的 `status` MUST 為 `N issue(s) found`
  AND exit code MUST 仍為 0（analyze 非 gate，同 drift 慣例）

#### Scenario: analyze-finding-catalog -- 10 種 finding 全集

**GIVEN** 針對每種 finding 構造的正／反 fixture
**WHEN** 使用者執行 `spectra analyze --json`
**THEN** 系統 MUST 支援且僅支援以下 finding keys（oracle binary 實測封閉集）：
      Coverage：`covMissingSpec`/`covMissingTask`/`covDeltaValidation`；
      Consistency：`conDesignNotInTasks`；
      Ambiguity：`ambNoScenario`/`ambAbstractScenario`/`ambWeakLanguage`；
      Gaps：`gapNoProposal`/`gapNoMainSpec`/`gapModifiedNotFound`
  AND 每種的觸發條件與 severity MUST 與 WP4 probe 實測結果一致（回填於 design.md）

#### Scenario: analyze-json-style -- snake_case 合約

**WHEN** 使用者執行 `spectra analyze --json`
**THEN** 頂層 keys MUST 為 snake_case（`change_id`/`artifacts_analyzed`/
      `artifacts_missing`），MUST NOT 「修正」為 camelCase
