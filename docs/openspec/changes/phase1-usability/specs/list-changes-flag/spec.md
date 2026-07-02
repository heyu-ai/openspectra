# List Changes Flag

<!-- capability: list-changes-flag -->

## US-002：golden-capture script 以 `list --changes --json` 取得 active changes

**Persona**：`scripts/capture-golden.sh`（oracle 校準腳本）與 CI 腳本，依賴原版 CLI 的 `list --changes --json` 介面。
**Action**：執行 `spectra list --changes --json`。
**Outcome**：取得與無 flag default 完全相同的 `{"changes":[...]}` JSON。

**Acceptance Criteria**：

- AC-002-1：`list --changes` 的輸出（human 與 `--json` 兩種）與無 flag 的 default 完全相同
- AC-002-2：`list --changes --specs` 與 `list --changes --parked` 被 clap 以衝突拒絕（exit code 非 0）
- AC-002-3：help 文字移除 "(not yet implemented)"，改為描述「顯式版的 default 行為」

#### Scenario: changes-flag-same-as-default -- `--changes` human 輸出與 default 完全相同

**GIVEN** 使用者位於含有 active changes 的 openspec 專案目錄，且尚未加任何 list 旗標
**WHEN** 使用者執行 `spectra list --changes`
**THEN** 系統 MUST 輸出與執行 `spectra list`（無旗標）完全相同的 human-readable 文字
  AND 系統 MUST NOT 因 `--changes` 而改變欄位順序、狀態計算或摘要內容

#### Scenario: changes-json-shape -- `--changes --json` 輸出結構與 default JSON 相同

**GIVEN** 使用者位於含有 active changes 的 openspec 專案目錄
**WHEN** 使用者執行 `spectra list --changes --json`
**THEN** 系統 MUST 輸出與 `spectra list --json`（無 `--changes`）byte-相同的 `{"changes":[{name, status, completedTasks, totalTasks, summary}]}` JSON
  AND 系統 MUST NOT 省略、重新排序或新增任何欄位

#### Scenario: changes-conflicts-specs -- `--changes` 與 `--specs` 同時提供被拒絕

**GIVEN** 使用者於同一次 CLI 呼叫中同時提供 `--changes` 與 `--specs` 兩個旗標
**WHEN** 使用者執行 `spectra list --changes --specs`
**THEN** 系統 MUST 由 clap 回報引數衝突錯誤並以非 0 exit code 結束
  AND 系統 MUST NOT 執行任何 list 邏輯或輸出 changes/specs 資料

#### Scenario: changes-conflicts-parked -- `--changes` 與 `--parked` 同時提供被拒絕

**GIVEN** 使用者於同一次 CLI 呼叫中同時提供 `--changes` 與 `--parked` 兩個旗標
**WHEN** 使用者執行 `spectra list --changes --parked`
**THEN** 系統 MUST 由 clap 回報引數衝突錯誤並以非 0 exit code 結束
  AND 系統 MUST NOT 執行任何 list 邏輯或輸出 changes/parked 資料

#### Scenario: help-text-no-longer-marks-changes-unimplemented -- help 文字移除「尚未實作」標記

**GIVEN** 使用者尚未執行任何 list 指令，僅查閱 CLI help
**WHEN** 使用者執行 `spectra list --help`
**THEN** 系統 MUST 在 `--changes` 旗標的說明文字中移除 "(not yet implemented)" 字樣
  AND 系統 MUST NOT 保留任何暗示 `--changes` 尚未生效或會被忽略的文字

#### Scenario: help-text-describes-default-equivalent -- help 文字描述 `--changes` 為顯式 default

**GIVEN** 使用者查閱 `spectra list --help` 的完整輸出
**WHEN** 使用者閱讀 `--changes` 旗標的說明文字
**THEN** 系統 MUST 以文字描述「`--changes` 顯式列出 active changes（即無旗標時的 default 行為）」
  AND 系統 SHOULD 於說明中提及其與 `--specs`/`--parked` 互斥
