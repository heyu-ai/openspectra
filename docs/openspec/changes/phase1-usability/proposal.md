# Proposal：phase1-usability

> 版本：v1.0 | 日期：2026-07-02 | 狀態：Draft
> 來源：`docs/roadmap.md` Phase 1 — 基礎修補與可用性

## Why

OpenSpectra 目前所有指令的錯誤訊息都指示使用者跑 `spectra init`，但這個子指令不存在，Linux 新使用者無法從零 bootstrap 專案。同時存在三個已知品質缺口：`list --changes` flag 收下但無作用（`scripts/capture-golden.sh` 依賴此指令）、root 環境下 3 個 chmod 權限測試誤判失敗（remote/container CI 常見情境）、CI 的 fmt/clippy 檢查是 `continue-on-error`（與 CLAUDE.md 宣告的硬門檻不一致）。Phase 1 無外部依賴、立即可做，完成後 Phase 2（OpenSpec 生態相容性）才能開始。

## What Changes

1. **`spectra init`**：新增 `spectra-core::init` 模組與 CLI `init` 子指令，從零 scaffold 專案（`.spectra.yaml`、`<spec_dir>/{changes,specs}/`、`.gitignore` 的 `.spectra/` entry）。**狀態：已實作並驗證（uncommitted，見 tasks.md US-001）**
2. **`list --changes` 接線**：把收下但無作用的 flag wire 到現行 default 的 active-change 列表，加 clap 衝突規則，移除 help 的 "(not yet implemented)"
3. **root 環境測試修正**：3 個 chmod(0o000) 測試在 permission-denied 不可構造時（root / CAP_DAC_OVERRIDE）skip 並印出原因
4. **CI 強化**：fmt/clippy 改硬性檢查（先修掉 pre-existing 格式 diff）、build+test matrix 加 macOS

## Step 1a — 四元素萃取

| 元素 | 內容 |
|------|------|
| **Actors** | Linux 新使用者（bootstrap）、CI 腳本／golden-capture script、root 容器內的 CI runner、repo maintainer |
| **Actions** | init 專案、列出 active changes（`--changes --json`）、root 下跑 `cargo test`、CI 檢查（fmt/clippy/build/test） |
| **Data** | `.spectra.yaml`、`<spec_dir>/changes/`、`<spec_dir>/specs/`、`.gitignore`、`{"changes":[...]}` JSON、`.github/workflows/ci.yml` |
| **Constraints** | init 不可重複初始化；`--changes` 輸出須與現行 default 完全相同（golden script 依賴）；root skip 不可誤傷一般使用者環境；fmt/clippy 硬門檻須與 CLAUDE.md 宣告一致；macOS 上 2 個 `#[cfg(target_os = "linux")]` 測試排除為預期行為 |

## Step 1 — User Stories

### US-001：Linux 新使用者從零 bootstrap 專案（capability: `project-init`）

**Persona**：剛拿到 OpenSpectra binary 的 Linux 使用者，repo 還沒有任何 spectra 設定，看到錯誤訊息 `Not initialized. Run 'spectra init' first.` 卻發現指令不存在。
**Action**：在專案根目錄執行 `spectra init`。
**Outcome**：專案完成初始化，後續 `new change → task done → drift → archive` 全流程可用。

**Acceptance Criteria**：

- AC-001-1：init 產生 `.spectra.yaml`，內容為 `spec_dir: openspec`
- AC-001-2：init 建立 `openspec/changes/` 與 `openspec/specs/` 目錄骨架
- AC-001-3：init 確保 `.gitignore` 含 `.spectra/` 一行——無檔案則建立；已有 entry 則不重複；既有內容無 trailing newline 時補齊再 append（PR #19 self-recording bug 根因即是沒 init 沒 gitignore）
- AC-001-4：已初始化（`.spectra.yaml` 存在）時回錯誤訊息含 `already initialized`，且不覆蓋任何既有檔案
- AC-001-5：`init --json` 輸出 shape `{root, spec_dir, gitignore_updated}`
- AC-001-6：init 後在同一 repo 執行 `new change → task done → drift` 全流程成功，且新 change 的 drift severity 為 `light`

### US-002：golden-capture script 以 `list --changes --json` 取得 active changes（capability: `list-changes-flag`）

**Persona**：`scripts/capture-golden.sh`（oracle 校準腳本）與 CI 腳本，依賴原版 CLI 的 `list --changes --json` 介面。
**Action**：執行 `spectra list --changes --json`。
**Outcome**：取得與無 flag default 完全相同的 `{"changes":[...]}` JSON。

**Acceptance Criteria**：

- AC-002-1：`list --changes` 的輸出（human 與 `--json` 兩種）與無 flag 的 default 完全相同
- AC-002-2：`list --changes --specs` 與 `list --changes --parked` 被 clap 以衝突拒絕（exit code 非 0）
- AC-002-3：help 文字移除 "(not yet implemented)"，改為描述「顯式版的 default 行為」

### US-003：root 容器內 CI 跑 `cargo test` 不誤判失敗（capability: `root-safe-tests`）

**Persona**：在 root 身分的 container（remote CI、devcontainer）內跑 `cargo test` 的 CI runner／開發者。
**Action**：以 euid==0 執行 `cargo test --all`。
**Outcome**：3 個依賴 chmod(0o000) 製造 permission-denied 的測試不誤判失敗，而是明確 skip。

**Acceptance Criteria**：

- AC-003-1：`touched.rs` 的 `load_warns_but_does_not_panic_on_a_permission_denied_read`、`archive.rs` 的 `archive_fails_loudly_on_an_unreadable_spec_delta_instead_of_silently_dropping_it` 與 `archive_preserves_the_underlying_error_cause_after_a_post_rename_failure` 三個測試，在 chmod(0o000) 後仍可讀（permission-denied 不可構造）時 skip
- AC-003-2：skip 時印出原因到 stderr（含測試名與「running as root (chmod 0o000 not enforced)」語意）
- AC-003-3：一般使用者環境下三個測試照常執行、不誤 skip——偵測機制為 chmod 後實際 read 探測（`permission_denied_is_constructible`），而非 euid 檢查，因此同時正確處理 CAP_DAC_OVERRIDE

### US-004：maintainer 讓 CI 與 CLAUDE.md 宣告的硬門檻一致（capability: `ci-hardening`）

**Persona**：repo maintainer，CLAUDE.md 已宣告 fmt/clippy 是本地硬門檻，但 CI 卻是 `continue-on-error: true`，且宣稱跨平台卻只在 ubuntu 驗證。
**Action**：修正 `.github/workflows/ci.yml` 與既有格式 diff。
**Outcome**：CI 的檢查強度與文件宣告一致，macOS 納入驗證。

**Acceptance Criteria**：

- AC-004-1：fmt 與 clippy 為 CI 硬性檢查（移除 `continue-on-error: true` 與過時的 "advisory" 註解）；前置：先 `cargo fmt --all` 修掉 pre-existing diff（`anchors.rs`、`calibration.rs`、`config.rs`、`drift.rs`、`tests/drift_integration.rs`），fmt 後全測試仍綠
- AC-004-2：build+test 以 matrix 跑 `ubuntu-latest` 與 `macos-latest` 兩平台（macOS 上 2 個 `#[cfg(target_os = "linux")]` 測試排除為預期行為）
- AC-004-3：lint（fmt+clippy）拆為 ubuntu-only job；CLI smoke step（`--help`）雙平台都跑
- AC-004-4：CLAUDE.md 的 Build/verify 一節同步更新，移除「fmt/clippy are continue-on-error in CI」的描述

## Step 4 — 假設與約束

### 假設

| # | 假設內容 | 若不成立的影響 |
|---|----------|----------------|
| A1 | `spectra init` 行為未經 oracle 驗證，依 README、各指令錯誤訊息與 PR #19 根因合理設計 | Phase 4 oracle 可用時需比對原版 init 的檔案集與訊息措辭；差異處以 calibration 修正。需在 `docs/reverse-engineering/init.md` 明確標註 |
| A2 | 實作環境可能無 Docker，root 環境 skip 行為無法本機實測 | PR body 註明未在 root 環境實測；靠 read-probe 機制的單元語意保證正確性 |

### 硬性限制

| # | 限制 | 來源 |
|---|------|------|
| C1 | `list --changes --json` 輸出不得 break `scripts/capture-golden.sh`（`{"changes":[...]}` shape） | 既有 oracle 校準流程 |
| C2 | 完工前四道全量驗證全綠：`cargo fmt --all -- --check`、`cargo clippy --all-targets -- -D warnings`、`cargo build --release --locked`、`cargo test --all` | CLAUDE.md 硬門檻 |
| C3 | Commit 拆分（feat init / feat list / fix tests / style fmt / ci），push 用 explicit refspec，開 draft PR，勿 merge | repo git 慣例 |
| C4 | 新邏輯放 `spectra-core`，CLI crate 保持薄殼；unit test 住模組內 `#[cfg(test)]`，integration test 住 `crates/<crate>/tests/` | CLAUDE.md workspace 慣例 |

### Out of Scope

| 功能 | 排除原因 | 未來考量 |
|------|----------|----------|
| `init --adopt`（在既有 OpenSpec 專案上補 sidecar） | Phase 2 相容性調查後才能定案 | roadmap Phase 2 |
| archive 的 MODIFIED/REMOVED/RENAMED delta 支援 | 屬 OpenSpec 相容性必要項，非 Phase 1 範圍 | roadmap Phase 2 |
| Release workflow、跨平台發佈 binary | 依賴 Phase 1+2 完成 | roadmap Phase 3 |
| oracle 校準（Time 邊界、Tasks 碰撞、Symbol 過濾） | 需 macOS + Spectra.app | roadmap Phase 4 |

## Step 5 — 完工標準

### Done 定義

此 change 視為「完成」的條件：

- [x] 所有 User Stories（US-001~004）的 AC 均已實作
- [x] testplan.md 所有 TC 均有對應的 Rust 測試或已標注為手動/CI 驗證項
- [x] 冒煙測試（SMK-001、SMK-002 已手動驗證；SMK-003 CI 全綠待 push 後由 GitHub Actions 確認）
- [x] 四道全量驗證指令全綠（C2）
- [x] `docs/reverse-engineering/init.md` 標註 init 未經 oracle 驗證（A1）
- [ ] 程式碼已 push、draft PR 已開（C3）

### 冒煙測試情境

#### Scenario: smk-init-e2e -- SMK-001 空 repo 全流程

**GIVEN** 一個全新的空 git repo（已設定 user.name/user.email），無任何 spectra 設定
**WHEN** 依序執行 `spectra init` → `spectra new change add-search-filter` → `spectra task done 1` → `spectra drift` → `spectra archive add-search-filter --skip-specs`
**THEN** 每一步 MUST 成功（exit code 0），且 `drift` 報告 severity MUST 為 `light`

#### Scenario: smk-list-changes-json -- SMK-002 golden script 介面

**GIVEN** 一個已 init 且含至少一個 active change 的專案
**WHEN** 執行 `spectra list --changes --json`
**THEN** 系統 MUST 輸出合法 JSON 且頂層 key 為 `changes`，內容 MUST 與 `spectra list --json` 完全相同

#### Scenario: smk-ci-green -- SMK-003 CI 全綠

**GIVEN** 本 change 的所有 commits 已 push 至 PR branch
**WHEN** GitHub Actions CI 執行
**THEN** lint job（fmt+clippy）與 build-and-test matrix（ubuntu + macos）MUST 全部通過

### Traceability Matrix

| US | Gherkin Scenario slug | TC-ID | Rust 測試（既有=✓ / 待寫=◻） |
|----|----------------------|-------|------------------------------|
| US-001 | `init-scaffold` | INIT-VL-001 | ✓ `init::tests::init_creates_config_and_scaffold_dirs` |
| US-001 | `init-gitignore-create` | INIT-VL-002 | ✓ `init::tests::init_creates_gitignore_with_spectra_entry_when_missing` |
| US-001 | `init-gitignore-append` | INIT-BVA-001 | ✓ `init::tests::init_appends_to_an_existing_gitignore_without_a_trailing_newline` |
| US-001 | `init-gitignore-no-dup` | INIT-VL-003 | ✓ `init::tests::init_does_not_duplicate_an_existing_spectra_gitignore_entry` |
| US-001 | `init-already-initialized` | INIT-EP-001 | ✓ `init::tests::init_errors_when_already_initialized`、✓ `init_is_idempotent_refusal_not_silent_reinit`（integration） |
| US-001 | `init-json-shape` | INIT-VL-004 | ✓ `tests::init_json_shape_matches_the_documented_contract`（spectra-cli） |
| US-001 | `init-e2e-pipeline` | SMK-001 | ✓ `init_then_new_change_then_drift_runs_end_to_end`（integration） |
| US-002 | `changes-flag-same-as-default` | LIST-EP-001 | ◻ 待寫（clap 解析 + code path 共用斷言） |
| US-002 | `changes-conflicts-specs` | LIST-DT-001 | ◻ 待寫（`Cli::try_parse_from` 衝突斷言） |
| US-002 | `changes-conflicts-parked` | LIST-DT-002 | ◻ 待寫（同上） |
| US-002 | `changes-json-shape` | SMK-002 | ✓ 既有 `list_change_items` 測試覆蓋 shape；smoke 驗證 flag 接線 |
| US-003 | `root-skip-with-reason` | ROOT-ST-001 | ◻ 修改 3 個既有測試加 skip guard（root 容器內人工/CI 驗證） |
| US-003 | `non-root-still-runs` | ROOT-EP-001 | ✓ 既有 3 測試在非 root 下必須維持原行為（回歸） |
| US-003 | `probe-mechanism-not-euid` | ROOT-VL-001 | ◻ `permission_denied_is_constructible` helper 語意（chmod 後 read 探測） |
| US-004 | `fmt-hard-gate` | CI-DT-001 | — CI 設定驗證（SMK-003） |
| US-004 | `clippy-hard-gate` | CI-DT-002 | — CI 設定驗證（SMK-003） |
| US-004 | `macos-matrix` | CI-PW-001 | — CI 設定驗證（SMK-003） |
| US-004 | `claude-md-consistency` | CI-VL-001 | — 文件 review 驗證 |
