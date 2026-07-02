# tasks.md — phase1-usability

> 優先序依據：US-001 是其他 phase 的前提（核心路徑，P1）；US-002/003/004 彼此獨立、無阻斷性依賴。
> `[P]` = 可平行執行；`[USn]` = 對應 User Story；無標記 = 有前序依賴。

## Phase 1：Setup

- [x] T001 建立 `docs/openspec/changes/phase1-usability/` spec 結構（本文件所在，已完成）

## Phase 2：Foundational

（無——四個 US 彼此獨立，無共用前置建設）

## Phase 3：User Stories

### US-001：`spectra init`（P1 — 核心路徑，Phase 2 adopt 與所有新使用者流程依賴它）✅ 已實作

**Story Goal**：Linux 新使用者能從零 bootstrap 專案。
**Test traceability**：AC-001-1~6 → TC INIT-VL-001~004, INIT-BVA-001, INIT-EP-001, SMK-001
  Verification: `cargo test -p spectra-core init::` + `cargo test -p spectra-core --test init_integration` + `cargo test -p spectra-cli init_json`

- [x] T010 [US1] `init` 模組（`init()`、`InitOutcome`、gitignore 三情境處理）— target: `crates/spectra-core/src/init.rs`
- [x] T011 [US1] `lib.rs` 掛載 `pub mod init` — target: `crates/spectra-core/src/lib.rs`
- [x] T012 [US1] CLI `Command::Init` + `cmd_init` + `init_json` shape helper（不走 `require_initialized`）— target: `crates/spectra-cli/src/main.rs`
- [x] T013 [US1] Unit tests ×5（scaffold、gitignore create/append/no-dup、already-initialized）— target: `crates/spectra-core/src/init.rs` `#[cfg(test)]`
- [x] T014 [US1] Integration tests ×2（init→new change→drift 全流程、重複 init 拒絕）— target: `crates/spectra-core/tests/init_integration.rs`
- [x] T015 [US1] CLI `init_json` shape unit test — target: `crates/spectra-cli/src/main.rs` `#[cfg(test)]`
- [x] T016 [US1] 手動 e2e 驗證（release binary，空 git repo：init → new change → task done → drift 成功、severity light）

### US-002：`list --changes` 接線（P2 — golden script 依賴，無阻斷性依賴）

**Story Goal**：`scripts/capture-golden.sh` 的 `list --changes --json` 介面可用。
**Test traceability**：AC-002-1~3 → TC LIST-EP-001, LIST-DT-001~002, SMK-002
  Verification: `cargo test -p spectra-cli list_changes`

- [x] T020 [P] [US2] `Command::List` 的 `changes` 欄位：`conflicts_with_all = ["specs", "parked"]`、doc comment 移除 "(not yet implemented)" — target: `crates/spectra-cli/src/main.rs`
- [x] T021 [US2] `run()` match arm 顯式 destructure `changes` 傳入 `cmd_list`（共用 default code path，見 design.md）— target: `crates/spectra-cli/src/main.rs`（依賴 T020）
- [x] T022 [US2] Unit tests：`Cli::try_parse_from` 衝突組合 `is_err()`（`--changes --specs`、`--changes --parked`）+ 合法組合 `changes == true` — target: `crates/spectra-cli/src/main.rs` `#[cfg(test)]`（依賴 T020）
- [x] T023 [US2] 手動驗證 SMK-002：`list --changes --json` 與 `list --json` 輸出完全相同

### US-003：root 環境測試修正（P2 — container CI 誤判，無阻斷性依賴）

**Story Goal**：root 容器內 `cargo test --all` 不誤判失敗。
**Test traceability**：AC-003-1~3 → TC ROOT-ST-001, ROOT-EP-001, ROOT-VL-001
  Verification: `cargo test -p spectra-core permission` （非 root 下照常執行）；root 容器（可選）：`docker run --rm -v $PWD:/w -w /w rust:latest cargo test -p spectra-core`

- [x] T030 [P] [US3] `permission_denied_is_constructible` helper + skip guard（含 stderr 原因）— target: `crates/spectra-core/src/touched.rs` `#[cfg(test)]`（`load_warns_but_does_not_panic_on_a_permission_denied_read`）
- [x] T031 [P] [US3] 同上 ×2 — target: `crates/spectra-core/src/archive.rs` `#[cfg(test)]`（`archive_fails_loudly_on_an_unreadable_spec_delta_instead_of_silently_dropping_it`、`archive_preserves_the_underlying_error_cause_after_a_post_rename_failure`）
- [x] T032 [US3] 非 root 回歸：3 個測試照常執行且通過（`cargo test --all` 全綠）；無 Docker，root 環境未實測（假設 A2，已於 commit message 註明）

### US-004：CI 強化（P2 — 品質門檻，無阻斷性依賴；建議最後做，fmt commit 才不會混入其他 diff）

**Story Goal**：CI 檢查強度與 CLAUDE.md 宣告一致，macOS 納入驗證。
**Test traceability**：AC-004-1~4 → TC CI-DT-001~002, CI-PW-001, CI-VL-001, SMK-003
  Verification: PR 的 GitHub Actions 全綠（lint + build-and-test ×2 平台）

- [x] T040 [US4] `cargo fmt --all` 修 pre-existing diff（5 檔；`calibration.rs:191` 行尾註解改獨立段落後再 fmt）；fmt 後 `cargo test --all` 全綠證明零行為變化 — target: `crates/**/*.rs`
- [x] T041 [US4] `ci.yml` 改雙 job 結構（`lint` ubuntu-only 硬門檻 + `build-and-test` matrix [ubuntu, macos] 含 smoke），刪 "advisory" 過時註解 — target: `.github/workflows/ci.yml`
- [x] T042 [P] [US4] CLAUDE.md Build/verify 一節同步（移除 continue-on-error 描述）— target: `CLAUDE.md`

## Phase 4：Polish

- [x] T050 [P] `docs/reverse-engineering/init.md`：標註 init 未經 oracle 驗證、列出產生的檔案集供日後 oracle 比對（假設 A1）— target: `docs/reverse-engineering/init.md`
- [x] T051 [P] README 檢視：補 `init`、`list --changes`，移除過期的 "not yet implemented" 提示 — target: `README.md`
- [x] T052 全量驗證四指令全綠（C2）：`cargo fmt --all -- --check` / `cargo clippy --all-targets -- -D warnings` / `cargo build --release --locked` / `cargo test --all`
- [x] T053 手動 e2e（SMK-001）：release binary 空 git repo `init → new change → task done → drift → archive` 全流程，severity light、exit 0
- [x] T054 Commit 拆分（C3）：`docs(openspec)` / `feat(init)` / `feat(list)` / `fix(tests)` / `style` / `ci` / `docs(init)` 獨立 commit；多行 commit message 用 `git commit -F <file>`；author email `2318485+howie@users.noreply.github.com`
- [ ] T055 Push（explicit refspec：`git push origin worktree-phase1-usability:worktree-phase1-usability`）+ 更新既有 draft PR #23（PR body 對照 US-001~004 逐項說明 + 驗證結果）；勿 merge
