# Project Init

<!-- capability: project-init -->

## US-001：Linux 新使用者從零 bootstrap 專案

**Persona**：剛拿到 OpenSpectra binary 的 Linux 使用者，repo 還沒有任何 spectra 設定。
**Action**：在專案根目錄執行 `spectra init`。
**Outcome**：專案完成初始化，後續 `new change → task done → drift → archive` 全流程可用。

### Scenarios

#### Scenario: init-scaffold -- 在乾淨專案執行 init 建立設定檔與目錄骨架

**GIVEN** 使用者在專案根目錄下，該目錄尚未存在 `.spectra.yaml`
**WHEN** 使用者執行 `spectra init`
**THEN** 系統 MUST 在專案根目錄產生 `.spectra.yaml`，內容為 `spec_dir: openspec`
  AND 系統 MUST 建立 `openspec/changes/` 目錄
  AND 系統 MUST 建立 `openspec/specs/` 目錄
  AND 系統 MUST NOT 覆蓋任何使用者既有檔案

#### Scenario: init-gitignore-create -- 專案無 .gitignore 時建立並寫入 entry

**GIVEN** 使用者在專案根目錄下，該目錄尚未存在 `.gitignore` 檔案，且尚未存在 `.spectra.yaml`
**WHEN** 使用者執行 `spectra init`
**THEN** 系統 MUST 建立 `.gitignore` 檔案
  AND 系統 MUST 在 `.gitignore` 中寫入 `.spectra/` 一行
  AND 系統 MUST 回報 `gitignore_updated` 為 `true`（於 `--json` 輸出時）

#### Scenario: init-gitignore-append -- 既有 .gitignore 無 trailing newline 時安全補齊後 append

**GIVEN** 使用者在專案根目錄下，`.gitignore` 已存在且內容結尾沒有 trailing newline，且尚未存在 `.spectra.yaml`
**WHEN** 使用者執行 `spectra init`
**THEN** 系統 MUST 先在既有內容補上 `\n`，再 append `.spectra/` 一行
  AND 系統 MUST NOT 破壞或截斷 `.gitignore` 既有內容
  AND 系統 MUST 回報 `gitignore_updated` 為 `true`（於 `--json` 輸出時）

#### Scenario: init-gitignore-no-dup -- .gitignore 已含 .spectra/ entry 時不得重複寫入

**GIVEN** 使用者在專案根目錄下，`.gitignore` 已存在且已含 `.spectra/` 一行，且尚未存在 `.spectra.yaml`
**WHEN** 使用者執行 `spectra init`
**THEN** 系統 MUST NOT 在 `.gitignore` 中重複寫入 `.spectra/`
  AND 系統 MUST 保持 `.gitignore` 內容不變
  AND 系統 MUST 回報 `gitignore_updated` 為 `false`（於 `--json` 輸出時）

**邊界值**（medium effort）：
- `.gitignore` 不存在 → THEN 系統 MUST 建立檔案並寫入 entry（見 init-gitignore-create）
- `.gitignore` 存在但缺 trailing newline → THEN 系統 MUST 補齊換行後 append（見 init-gitignore-append）
- `.gitignore` 存在且已含 entry（含 trailing newline） → THEN 系統 MUST NOT 重複寫入（見 init-gitignore-no-dup）
- `.gitignore` 存在且已含 entry 但格式為 `.spectra/  ` （尾端空白）→ 系統 SHOULD 視為已有 entry，MUST NOT 重複寫入

#### Scenario: init-already-initialized -- 重複執行 init 時回錯誤且不覆蓋任何檔案

**GIVEN** 使用者在專案根目錄下，該目錄已存在 `.spectra.yaml`（先前已成功執行過 `spectra init`）
**WHEN** 使用者再次執行 `spectra init`
**THEN** 系統 MUST 回傳錯誤，錯誤訊息 MUST 包含 `already initialized`
  AND 系統 MUST NOT 覆蓋 `.spectra.yaml`
  AND 系統 MUST NOT 修改 `openspec/changes/`、`openspec/specs/` 或 `.gitignore` 的既有內容

#### Scenario: init-json-shape -- init --json 輸出符合固定 shape

**GIVEN** 使用者在專案根目錄下，該目錄尚未存在 `.spectra.yaml`
**WHEN** 使用者執行 `spectra init --json`
**THEN** 系統 MUST 輸出合法 JSON，且欄位 MUST 恰為 `root`、`spec_dir`、`gitignore_updated`
  AND `root` 欄位 MUST 為專案根目錄的絕對路徑字串
  AND `spec_dir` 欄位 MUST 為 `openspec`
  AND `gitignore_updated` 欄位 MUST 為布林值，如實反映 `.gitignore` 是否被建立或修改
  AND 系統 MUST NOT 在 `--json` 模式下額外輸出非 JSON 的人類可讀文字到 stdout

#### Scenario: init-e2e-pipeline -- init 後完整流程 new change → task done → drift 可成功執行

**GIVEN** 使用者在乾淨專案根目錄下已成功執行 `spectra init`
**WHEN** 使用者依序執行 `spectra new change`、完成該 change 的 task（task done）、再執行 `spectra drift`
**THEN** 系統 MUST 讓上述三個指令全部成功執行，不因初始化狀態不足而報錯
  AND 系統 MUST 回報該筆新 change 的 drift severity 為 `light`
  AND 系統 MUST NOT 要求使用者在 `init` 之外額外手動建立 `.spectra.yaml` 或 `openspec/` 目錄
