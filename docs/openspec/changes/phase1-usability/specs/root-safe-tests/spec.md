# Root-Safe 權限測試

<!-- capability: root-safe-tests -->

## US-003：root 容器內 CI 跑 `cargo test` 不誤判失敗

**Persona**：在 root 身分的 container（remote CI、devcontainer）內跑 `cargo test` 的 CI runner／開發者。
**Action**：以 euid==0 執行 `cargo test --all`。
**Outcome**：3 個 chmod 測試不誤判失敗，而是明確 skip。

#### Scenario: root-skip-with-reason -- root 身分下 chmod(0o000) 不可構造 permission-denied 時測試明確 skip

**GIVEN** 測試以 root（euid==0，或具備 CAP_DAC_OVERRIDE）身分執行，且對目標測試檔已 chmod(0o000)
**WHEN** 測試呼叫 `permission_denied_is_constructible(path)` 探測該路徑
**THEN** 系統 MUST 偵測到 `std::fs::read(path)` 仍然成功（`.is_err()` 回傳 `false`）
  AND 系統 MUST 將該測試判定為 skip 而非失敗
  AND 系統 MUST NOT 讓測試以 assertion failure 的方式結束
  AND 系統 MUST 在 stderr 印出包含測試名稱與「running as root (chmod 0o000 not enforced)」語意的訊息

**邊界值**（medium effort）：
- `permission_denied_is_constructible` 回傳 `false`（root/CAP_DAC_OVERRIDE 情境）→ THEN 測試 SHALL 印出 skip 原因後直接 return，不執行後續斷言
- `permission_denied_is_constructible` 回傳 `true`（一般使用者情境）→ THEN 測試 SHALL 照常執行完整斷言流程

#### Scenario: non-root-still-runs -- 一般使用者環境下三個測試照常執行不誤 skip

**GIVEN** 測試以一般（非 root、無 CAP_DAC_OVERRIDE）使用者身分執行，且已對目標測試檔 chmod(0o000)
**WHEN** 測試呼叫 `permission_denied_is_constructible(path)` 探測該路徑
**THEN** 系統 MUST 偵測到 `std::fs::read(path)` 回傳 `Err`（`.is_err()` 回傳 `true`）
  AND 系統 MUST 繼續執行原本的測試邏輯（`load_warns_but_does_not_panic_on_a_permission_denied_read`、`archive_fails_loudly_on_an_unreadable_spec_delta_instead_of_silently_dropping_it`、`archive_preserves_the_underlying_error_cause_after_a_post_rename_failure` 三者皆同）
  AND 系統 MUST NOT 印出 skip 訊息或提前 return
  AND 系統 MUST 對 permission-denied 情境下的既有行為斷言（如 warn-not-panic、fail-loudly、error cause 保留）維持原本結果不變

**邊界值**（medium effort）：
- 非 root 環境、chmod 生效 → THEN 三個測試 SHALL 全數執行到既有斷言並通過（回歸不變）
- 非 root 環境、但檔案系統不支援權限位元（如某些網路掛載）→ THEN 探測 SHALL 回傳 `false` 並依 root-skip-with-reason 流程 skip，而非誤判為失敗

#### Scenario: probe-mechanism-not-euid -- 偵測機制必須是 read 探測而非 euid 檢查

**GIVEN** 一個已 chmod(0o000) 的測試檔案路徑，執行環境的 euid 未知或無法透過標準函式庫直接取得（不引入 `libc` 等外部 dependency）
**WHEN** `permission_denied_is_constructible(path)` 被呼叫
**THEN** 系統 MUST 僅以 `std::fs::read(path).is_err()` 作為唯一判斷依據，不得檢查 euid 或使用者身分
  AND 系統 MUST 同時涵蓋一般 root（euid==0）與具備 CAP_DAC_OVERRIDE 的非 root-uid 容器情境（兩者皆會使 read 成功、探測回傳 `false`）
  AND 系統 MUST NOT 依賴任何需要額外 crate（如 `libc`）才能取得的 euid／capability 資訊

**邊界值**（medium effort）：
- 環境為 CAP_DAC_OVERRIDE 但 euid != 0（容器常見設定）→ THEN 探測 SHALL 依然回傳 `false`（因為 read 實際成功），與 root-skip-with-reason 行為一致
- 環境為一般使用者且無任何特殊 capability → THEN 探測 SHALL 回傳 `true`，與 non-root-still-runs 行為一致
