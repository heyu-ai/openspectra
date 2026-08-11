# Workflow Status

<!-- capability: workflow-status -->

## US-101：agent 以 status 取得 DAG 下一步

**Persona**：sdd plugin 驅動的 agent，在 change 工作流各階段間需要知道哪些 artifact 可做。
**Action**：執行 `spectra status --json`。
**Outcome**：取得每個 artifact 的 `done`/`ready`/`blocked` 狀態與 `isComplete`。

### Scenarios

#### Scenario: status-empty-change -- 新建 change 只有 proposal ready

**GIVEN** 一個以 `spectra new change` 建立、尚無任何 artifact 檔的 change
**WHEN** 使用者執行 `spectra status --json`
**THEN** 系統 MUST 輸出 `proposal` 為 `ready`
  AND `design`、`specs` MUST 為 `blocked` 且 `missingDeps` 為 `["proposal"]`
  AND `tasks` MUST 為 `blocked` 且 `missingDeps` 為 `["specs"]`
  AND `isComplete` MUST 為 `false`

#### Scenario: status-file-existence -- done 僅由檔案存在性決定

**GIVEN** change 內已有 proposal.md、design.md、tasks.md 與至少一個 `specs/**/*.md`
**WHEN** 使用者刪除整個 `specs/` 目錄後執行 `spectra status --json`
**THEN** `specs` MUST 回到 `ready`（proposal 仍 done）
  AND `tasks` MUST 維持 `done`（檔案仍存在，狀態不 cascade）
  AND `isComplete` MUST 為 `false`

#### Scenario: status-complete -- 全部 artifact 就緒

**GIVEN** change 內四種 artifact 檔皆存在
**WHEN** 使用者執行 `spectra status --json`
**THEN** 全部 artifacts MUST 為 `done` 且 MUST NOT 含 `missingDeps` 欄位
  AND `isComplete` MUST 為 `true`
  AND `applyRequires` MUST 為 `["tasks"]`

#### Scenario: status-json-contract -- JSON 命名與 human 輸出對齊 oracle

**GIVEN** 任一已初始化 change
**WHEN** 使用者執行 `spectra status --json` 與 `spectra status`
**THEN** JSON keys MUST 為 camelCase（`changeName`/`schemaName`/`isComplete`/`applyRequires`）
  AND human 輸出 MUST 以 `✓`（done）／`○`（ready）／`✗`（blocked）標記，
      blocked 項下一行 MUST 為 `    blocked by: <deps>`
