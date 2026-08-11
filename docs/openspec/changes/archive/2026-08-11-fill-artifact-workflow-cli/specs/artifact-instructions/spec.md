# Artifact Instructions

<!-- capability: artifact-instructions -->

## US-103：agent 取得 artifact 撰寫指令與模板

**Persona**：sdd plugin 驅動的 agent，準備撰寫下一個 artifact。
**Action**：執行 `spectra instructions proposal --json`。
**Outcome**：取得該 artifact 的 instruction 文字、模板、依賴狀態。

### Scenarios

#### Scenario: instructions-artifact-json -- 單一 artifact 模式

**GIVEN** 一個已初始化的 change
**WHEN** 使用者執行 `spectra instructions proposal --json`
**THEN** 輸出 MUST 為 pretty JSON，keys 為 `changeName, artifactId, schemaName,
      changeDir, outputPath, description, instruction, locale, template,
      dependencies, unlocks`
  AND `instruction` 與 `template` MUST 與內建 spec-driven schema 常數逐字一致
  AND `dependencies` MUST 為 `{id, done, path, description}` 物件陣列，
      `done` 反映該依賴當下的檔案存在性

#### Scenario: instructions-apply -- 無參數／apply 形態

**GIVEN** change 內四種 artifact 皆存在且 tasks.md 有未完成 checkbox
**WHEN** 使用者執行 `spectra instructions --json`（或 `instructions apply --json`）
**THEN** 輸出 MUST 含 `contextFiles`（四 artifact 的絕對路徑／glob）
  AND `progress` MUST 為 `{total, complete, remaining}` 且與 tasks.md checkbox 一致
  AND `tasks[]` MUST 為 `{id, description, done, parallel}`（id 為字串序號）
  AND MUST 含 `preflight.status` 與 `staleness.{daysOld,isStale}`

#### Scenario: instructions-human -- human 輸出

**WHEN** 使用者執行 `spectra instructions`（無 `--json`，apply 形態）
**THEN** 輸出 MUST 依序含 `Change:`、`Schema:`、`State:`、`Progress: N/M complete`、
      `Tasks:`（`○`/`✓` 條列）、`Instruction:` 區塊
