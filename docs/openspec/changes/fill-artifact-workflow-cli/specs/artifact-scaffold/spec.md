# Artifact Scaffold

<!-- capability: artifact-scaffold -->

## US-102：agent 以 stdin 建立 artifact

**Persona**：sdd plugin 驅動的 agent，已在 context 中產出 artifact 內容。
**Action**：`cat content.md | spectra new artifact proposal --stdin --json`。
**Outcome**：內容寫入正確路徑並通過 per-type 驗證。

### Scenarios

#### Scenario: scaffold-stdin -- 以 stdin 內容建立 artifact

**GIVEN** 一個尚無 proposal.md 的 change
**WHEN** 使用者以 `--stdin` 管線輸入含 `## Why` 的內容執行 `spectra new artifact proposal --stdin --json`
**THEN** 系統 MUST 將內容寫入 `<change>/proposal.md`
  AND MUST 輸出**單行 compact JSON** `{"artifact","change","path","status":"created","validated":true,"warnings":[]}`

#### Scenario: scaffold-spec-capability -- spec 型需要 capability 名

**GIVEN** 任一 change
**WHEN** 使用者執行 `spectra new artifact spec`（未給 capability）
**THEN** 系統 MUST 以 exit 1 失敗，stderr 為
      `Error: Capability name is required for spec type. Usage: spectra new artifact spec <capability> --change <name>`
**WHEN** 使用者執行 `spectra new artifact spec demo-cap --stdin`
**THEN** 系統 MUST 寫入 `<change>/specs/demo-cap/spec.md`

#### Scenario: scaffold-exists-force -- 已存在時擋下，--force 覆蓋

**GIVEN** change 內 proposal.md 已存在
**WHEN** 使用者執行 `spectra new artifact proposal --stdin`
**THEN** 系統 MUST 以 exit 1 失敗，stderr 為
      `Error: Artifact already exists: <絕對路徑>. Use --force to overwrite`
**WHEN** 改加 `--force` 重跑
**THEN** 系統 MUST 覆蓋檔案並回 `"status":"created"`

#### Scenario: scaffold-validation -- per-type 內容驗證失敗即擋下

**GIVEN** 任一 change
**WHEN** 使用者以 `--stdin` 輸入不含 `## Why`/`## Problem`/`## Summary` 的內容建立 proposal
**THEN** 系統 MUST 以 exit 1 失敗，stderr 為
      `Error: Proposal must contain a ## Why, ## Problem, or ## Summary section`
  AND MUST NOT 寫入任何檔案

#### Scenario: scaffold-unknown-type -- 未知 type

**WHEN** 使用者執行 `spectra new artifact bogus`
**THEN** 系統 MUST 以 exit 1 失敗，stderr 為
      `Error: Unknown artifact type 'bogus'. Valid types: proposal, design, tasks, spec`

#### Scenario: scaffold-template -- 無 --stdin 時以模板建檔

**GIVEN** change 內無 design.md
**WHEN** 使用者執行 `spectra new artifact design`
**THEN** 系統 MUST 以 design 的內建模板（同 `instructions design` 的 `template` 欄位）建檔

#### Scenario: new-change-no-scaffold -- new change 不再預建 artifact 檔（BREAKING）

**GIVEN** 已初始化的專案
**WHEN** 使用者執行 `spectra new change my-feature`
**THEN** 系統 MUST 只建立 change 目錄與 `.openspec.yaml`
      （含 `schema: spec-driven`、`created: YYYY-MM-DD`、`created_by`）
  AND MUST NOT 建立 proposal.md／design.md／tasks.md
