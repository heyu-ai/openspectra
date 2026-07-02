# CI Hardening

<!-- capability: ci-hardening -->

## US-004：maintainer 讓 CI 與 CLAUDE.md 宣告的硬門檻一致

**Persona**：repo maintainer，CLAUDE.md 已宣告 fmt/clippy 是本地硬門檻，但 CI 卻是
`continue-on-error: true`，且宣稱跨平台卻只在 ubuntu 驗證。
**Action**：修正 `.github/workflows/ci.yml` 與既有格式 diff。
**Outcome**：CI 的檢查強度與文件宣告一致，macOS 納入驗證。

### AC-004-1：fmt 與 clippy 為 CI 硬性檢查

#### Scenario: fmt-hard-gate -- cargo fmt --check 失敗時 CI MUST 阻擋合併

**GIVEN** `.github/workflows/ci.yml` 的 `lint` job 已移除 fmt step 的
`continue-on-error: true` 與過時的 "advisory" 註解，且 repo 內五個
pre-existing 格式 diff 檔案（`anchors.rs`、`calibration.rs`、`config.rs`、
`drift.rs`、`tests/drift_integration.rs`）已先以 `cargo fmt --all` 修正
**WHEN** PR 貢獻者推送一個含未格式化 Rust 程式碼（例如縮排或空白不符
`rustfmt` 規則）的 commit 觸發 CI
**THEN** 系統 MUST 執行 `cargo fmt --all -- --check` 並回傳非零結束碼
  AND 系統 MUST 將 `lint` job 標記為失敗，阻擋該 PR 合併
  AND 系統 MUST NOT 略過此失敗結果（不得因 `continue-on-error` 而顯示為通過）

#### Scenario: fmt-hard-gate-baseline-clean -- 修正 pre-existing diff 後全測試仍綠

**GIVEN** 五個 pre-existing 格式 diff 檔案已套用 `cargo fmt --all`，且此格式化
變更不涉及任何邏輯修改
**WHEN** CI 針對此次格式化 commit 執行完整驗證流程
**THEN** 系統 MUST 回報 `cargo fmt --all -- --check` 通過（結束碼 0）
  AND 系統 MUST 回報 `cargo test --all` 全數通過，且測試案例數與格式化前相同
  AND 系統 MUST NOT 出現任何因格式化而導致的行為變化（測試結果差異）

#### Scenario: clippy-hard-gate -- clippy 警告時 CI MUST 阻擋合併

**GIVEN** `.github/workflows/ci.yml` 的 `lint` job 已移除 clippy step 的
`continue-on-error: true`，且 `cargo clippy --all-targets -- -D warnings`
在目前 main 分支上乾淨無警告
**WHEN** PR 貢獻者推送一個會觸發 clippy 警告（例如未使用的變數或多餘的
`clone()`）的 commit 觸發 CI
**THEN** 系統 MUST 執行 `cargo clippy --all-targets -- -D warnings` 並因
`-D warnings` 使結束碼為非零
  AND 系統 MUST 將 `lint` job 標記為失敗，阻擋該 PR 合併
  AND 系統 MUST NOT 將此警告降級為僅供參考的訊息

### AC-004-2：build+test 以 matrix 跑 ubuntu-latest 與 macos-latest

#### Scenario: macos-matrix -- build-and-test job 在雙平台皆須通過

**GIVEN** `.github/workflows/ci.yml` 的 `build-and-test` job 設定
`strategy.matrix.os: [ubuntu-latest, macos-latest]`
**WHEN** PR 觸發 CI 執行 `cargo build --release --locked` 與
`cargo test --all`
**THEN** 系統 MUST 在 `ubuntu-latest` runner 上執行完整測試套件並回報全數
通過（含 2 個 `#[cfg(target_os = "linux")]` 專屬測試）
  AND 系統 MUST 在 `macos-latest` runner 上執行測試套件並回報全數通過
（排除 2 個 linux-only 測試，此差異視為預期行為）
  AND 系統 MUST NOT 因平台間測試數量差異而將 macOS job 標記為失敗

**邊界值**（medium effort，用相對關係而非硬編絕對數字——絕對測試數會隨新增測試持續變動，見 testplan.md「Partially Covered」對此風險的說明）：
- ubuntu 測試通過數 = macOS 測試通過數 + 2（linux-only 測試差額）→ THEN 系統 SHALL 判定為通過
- ubuntu 測試通過數 ≠ macOS 測試通過數 + 2（差額不是預期的 2，代表某平台有非預期的測試流失）→ THEN 系統 SHALL 判定為失敗

#### Scenario: macos-matrix-build-failure -- macOS 專屬編譯錯誤 MUST 阻擋合併

**GIVEN** `build-and-test` job 已設定 `matrix.os` 涵蓋 `macos-latest`，且
程式碼含僅在 macOS 工具鏈上才會觸發的編譯錯誤（例如平台特定 API 誤用）
**WHEN** CI 在 `macos-latest` runner 上執行 `cargo build --release --locked`
**THEN** 系統 MUST 回報該 matrix leg 建置失敗
  AND 系統 MUST 將整體 `build-and-test` job 標記為失敗，即使 `ubuntu-latest`
leg 通過
  AND 系統 MUST NOT 僅因 ubuntu leg 綠燈就允許 PR 合併

### AC-004-3：lint 拆為 ubuntu-only job；smoke 雙平台皆跑

#### Scenario: lint-ubuntu-only -- lint job 僅在 ubuntu-latest 執行一次

**GIVEN** `.github/workflows/ci.yml` 已將 fmt 與 clippy 檢查獨立為 `lint`
job，且該 job 未設定 `matrix.os`（固定跑在 `ubuntu-latest`）
**WHEN** PR 觸發 CI 工作流程
**THEN** 系統 MUST 僅執行一次 `lint` job（於 `ubuntu-latest`），不重複於
`macos-latest` 執行 fmt/clippy 檢查
  AND 系統 MUST 讓 `lint` job 與 `build-and-test` matrix job 平行執行以節省
CI 總時長
  AND 系統 MUST NOT 在 `build-and-test` 的 macOS matrix leg 中重複執行
`cargo fmt --check` 或 `cargo clippy`

#### Scenario: smoke-dual-platform -- CLI smoke test 雙平台皆須執行並通過

**GIVEN** `build-and-test` job 的 smoke step（`./target/release/spectra --help`）
已定義於 matrix 內、不限定單一 `os` 條件
**WHEN** CI 分別在 `ubuntu-latest` 與 `macos-latest` 完成 `cargo build --release --locked`
**THEN** 系統 MUST 在兩個平台上皆執行 `./target/release/spectra --help` 並確認結束碼為 0
  AND 系統 MUST 將任一平台 smoke step 失敗視為該 matrix leg 失敗
  AND 系統 MUST NOT 讓 smoke step 僅限定在 `ubuntu-latest` 執行

### AC-004-4：CLAUDE.md 的 Build/verify 段落同步更新

#### Scenario: claude-md-consistency -- CLAUDE.md 移除過時的 continue-on-error 描述

**GIVEN** 專案根目錄 `CLAUDE.md` 的 "Build / verify" 一節先前記載
"`fmt`/`clippy` are `continue-on-error` in CI"，與已更新的 `ci.yml` 行為不符
**WHEN** maintainer 完成 `ci.yml` 的 lint/build-and-test job 拆分與硬門檻化後
更新 `CLAUDE.md`
**THEN** 系統 MUST 移除 "fmt/clippy are continue-on-error in CI" 的描述
  AND 系統 MUST 在文件中明確記載 fmt 與 clippy 現為 CI 的硬性檢查
（與本地行為一致）
  AND 系統 MUST NOT 保留任何暗示 fmt/clippy 檢查結果可被忽略的字句

#### Scenario: claude-md-consistency-stale-doc -- 文件未同步時 MUST 視為不一致

**GIVEN** `ci.yml` 已更新為 fmt/clippy 硬門檻，但 `CLAUDE.md` 的
"Build / verify" 段落仍保留舊有 "continue-on-error" 字樣
**WHEN** reviewer 檢視此 PR 的文件與程式碼變更是否一致
**THEN** 系統 MUST 判定文件與實際 CI 行為不一致，要求 PR 補齊
`CLAUDE.md` 更新後才可合併
  AND 系統 MUST NOT 將此文件落差視為可合併的次要瑕疵
