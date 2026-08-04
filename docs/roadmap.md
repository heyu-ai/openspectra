# OpenSpectra 分階段開發與測試計畫 (Roadmap)

## Context（背景）

OpenSpectra 是 closed-source `spectra` CLI 的 Rust 反組譯重實作。上游 owner 無力維護、bug 回報無回應，因此本專案目標是產出一個：

1. **開源、可在 Linux 上執行**的替代品（原版是 macOS arm64 專屬 app bundle）
2. **支援 openspec** 格式的 spec-driven-development 工作流
3. 對原版行為**先忠實重現、再漸進改進**（校準期與 oracle 一致以便驗證，之後以 opt-in 方式修正已知缺陷）

### 現況摘要（2026-07-11）

**Phase 進度以 GitHub issue 追蹤（每個 Phase 一個 epic issue，與本文件雙向連結）**：Phase 1 ✅ [#30](https://github.com/howie/openspectra/issues/30)、Phase 2 ✅ [#26](https://github.com/howie/openspectra/issues/26)、Phase 3 ✅ [#27](https://github.com/howie/openspectra/issues/27)（v0.2.1 發佈）皆已完成；Phase 4 [#28](https://github.com/howie/openspectra/issues/28)、Phase 5 [#29](https://github.com/howie/openspectra/issues/29) open。

**已完成（drift 核心經 oracle 驗證，已合併 PR：#1、#2、#13–#21）**：`drift`（四維度評分、JSON schema 逐欄位吻合、exit code gate）、`list`（--specs/--parked）、`show`（change+spec）、`park`/`unpark`、`new change`、`task done`、`archive`（ADDED delta）。

**Phase 1 已完成（PR #23）**：`spectra init`、`list --changes` 接線、root 環境 3 個 chmod 權限測試改用 runtime probe 跳過、CI fmt/clippy 硬門檻 + ubuntu/macOS build+test matrix。（`spectra init` 為 oracle-unverified，見 `docs/reverse-engineering/init.md`。）

**Open issues**：

- ~~#8 Symbol-anchor 縮窄過濾~~ **已解**（#119）：不是語意 filter，就是 over-cap 的
  per-category 抽樣。實測 30 個 Symbol 候選全留、83 個留 `floor(i*83/12)` 的 12 個；
  「12 of ~83」就是 `ANCHOR_SAMPLE_PER_CATEGORY` of 83，不需反組譯
- #9 Tasks 碰撞偵測（無 positive oracle 樣本，偵測整個 gated off）
- #10 Time 維度日數邊界（5↔7、19↔25 內插，60 天是猜的）
- #11 CliFlag 永遠 broken：忠實重現 vs 可設定目標 CLI（設計決策）
- #12 `Resolver::broken` 逐 anchor fork `git grep`（效能，最多 50 次子程序）

**Phase 1 已消除的缺口（PR #23）**：~~`spectra init` 不存在~~、~~`list --changes` 收下但無作用~~、~~root 下 3 個 chmod 權限測試誤判~~、~~無跨平台 CI matrix~~。

**Phase 2/3 已消除的缺口**：~~archive 不支援 MODIFIED/REMOVED/RENAMED spec delta~~（PR #32 已補）、~~無 release 流程~~（Phase 3 / v0.2.1）、~~無 OpenSpec 生態相容性驗證~~（PR #32）。

**尚存缺口（對應後續 Phase）**：

- 無 `unarchive`（快照/還原；已知限制，見 [`docs/reverse-engineering/archive.md`](reverse-engineering/archive.md)）
- oracle 校準未收尾（Phase 4，[#28](https://github.com/howie/openspectra/issues/28)，子項 #8/#9）
- CliFlag resolution 決策待定（Phase 5，[#11](https://github.com/howie/openspectra/issues/11)）

### 計畫假設

| 決策點 | 假設 |
|---|---|
| openspec 支援範圍 | 目標為能在真實 OpenSpec (Fission-AI) 專案上執行；Phase 2 先做格式差異調查再決定相容層大小 |
| 忠實 vs 改進 | 先忠實、後改進：預設行為與 oracle 一致，改進以設定 opt-in |
| oracle 環境 | 不假設隨時有 macOS + Spectra.app；oracle 依賴的工作獨立成 Phase 4，無 oracle 時可跳過並保守標註 |
| 發佈形式 | GitHub Releases 二進位（Linux musl 靜態 + macOS）為主，crates.io 為次 |

---

## Phase 1 — 基礎修補與可用性（無外部依賴，立即可做）✅ 完成（PR #23，追蹤 [#30](https://github.com/howie/openspectra/issues/30)）

目標：讓 Linux 使用者能從零開始使用，並消除已知的殼 flag / 測試環境問題。

1. **`spectra init`**（新 issue）
   - `spectra-core` 新增 `init` 模組：產生 `.spectra.yaml`（`spec_dir: openspec`）、建立 `<spec_dir>/changes/`、`<spec_dir>/specs/` 骨架、把 `.spectra/` 加進 `.gitignore`（PR #19 發現的 self-recording bug 根因就是沒 init 沒 gitignore）
   - 若能取得 oracle 輸出，比對原版 `init` 的檔案集與訊息措辭；不能就依 README/錯誤訊息合理設計並在 `docs/reverse-engineering/` 標註未經 oracle 驗證
   - 測試：unit（冪等、已初始化時的錯誤）、integration（init → new change → drift 全流程）
2. **`list --changes` 接線**（新 issue）
   - 語意即現行預設的 active-change 列表（原版 `capture-golden.sh` 用 `list --changes --json`）；wire 後移除 help 的 "(not yet implemented)"
   - 測試：unit + `--json` shape
3. **root 環境測試修正**（新 issue）
   - 三個 chmod(0o000) 測試在 root 下跳過（執行期偵測 euid==0 即 skip，並印出 skip 原因），或改用其他機制製造 IO error
   - 檔案：`crates/spectra-core/src/touched.rs:370`（`set_permissions(..., 0o000)` 呼叫處）、`archive.rs` 兩處
4. **CI 強化**
   - `fmt`/`clippy` 從 `continue-on-error: true` 改為硬性檢查（CLAUDE.md 已當硬門檻，CI 應一致）
   - 加 macOS runner 到 build+test matrix（本專案宣稱跨平台，需雙平台驗證）

**驗證**：`cargo fmt --check` / `clippy -D warnings` / `build --release --locked` / `test --all` 全綠（含 root 容器內）；手動 e2e：空目錄 `init → new change → task done → drift → archive` 全流程在 Linux 通過。

## Phase 2 — OpenSpec 生態相容性（追蹤 [#26](https://github.com/howie/openspectra/issues/26)）

目標：確認/達成「在真實 OpenSpec 專案上直接可用」。

1. **格式差異調查**（研究，先做）
   - 對照 Fission-AI OpenSpec 的實際目錄約定（`openspec/project.md`、`changes/<name>/{proposal,design,tasks}.md`、`specs/<capability>/spec.md`、delta 標記 `## ADDED/MODIFIED/REMOVED/RENAMED Requirements`）與現有實作
   - 產出：`docs/openspec-compat.md` 差異矩陣（哪些直接相容、哪些需要適配、哪些是 spectra 專屬如 `.openspec.yaml` sidecar、`.spectra/` 目錄）
   - 關鍵問題：OpenSpec 專案沒有 `.spectra.yaml` 與 `.openspec.yaml`——`drift` 在其上要能跑，可能需要 (a) `init --adopt` 補齊 sidecar，或 (b) 無 sidecar 時的 fallback（如以 git log 推 `created` 日期）
2. **相容性實作**（依調查結果開 issues）
   - 最小方案：`spectra init` 在既有 `openspec/` 目錄上 adopt（不覆蓋既有內容，補 `.spectra.yaml` 與缺失 metadata）
   - archive 的 MODIFIED/REMOVED/RENAMED delta 支援（OpenSpec 工作流會產生這些；現在直接報錯）——這是相容性的必要項，不只是 nice-to-have
3. **相容性測試**
   - `crates/spectra-cli/tests/openspec_compat.rs`：以真實 OpenSpec 範例專案為 fixture（vendored 快照），跑 `init --adopt → list → drift → archive` 斷言不 panic、輸出合理
   - 若 OpenSpec CLI 可在 CI 安裝（npm），加一個 optional job：OpenSpec CLI 建專案 → openspectra 消化

**驗證**：fixture 測試進 CI；手動用 OpenSpec CLI 建一個真實專案，openspectra 全指令跑通。

## Phase 3 — Linux 發佈工程（追蹤 [#27](https://github.com/howie/openspectra/issues/27)）

目標：使用者能一行安裝、CI 能直接拿來當 drift gate。

1. **Release workflow**（`.github/workflows/release.yml`，tag `v*` 觸發）
   - 目標平台：`x86_64-unknown-linux-musl`、`aarch64-unknown-linux-musl`（靜態連結，唯一 runtime 依賴是 PATH 上的 `git`，與現行設計一致）、`x86_64/aarch64-apple-darwin`
   - 產物：tar.gz + sha256、自動生成 release notes；可評估用 `cargo-dist` 減少手寫 YAML
2. **crates.io 發佈**：`spectra-core` + `spectra-cli` 補 metadata（license、description、repository），`cargo publish` 納入 release 流程
3. **版本策略**：`0.x` semver；CHANGELOG 記錄與 oracle 的刻意分歧（配合 Phase 4/5 的忠實→改進切換）
4. **（可選）Docker image**：`FROM alpine` + musl binary + git，發到 GHCR，README 給 GitHub Actions drift-gate 範例

**驗證**：打 `v0.1.0-rc` tag 走一次完整 release；在乾淨的 x86_64 與 aarch64 容器裡下載 binary 跑 e2e smoke（`init → new change → drift`）。

## Phase 4 — Oracle 校準收尾（需 macOS + Spectra.app；無 oracle 則降級處理）（追蹤 [#28](https://github.com/howie/openspectra/issues/28)，子項 #8/#9/#10）

目標：把「猜的常數」變成「量測的常數」。工作模式：本專案產生校準腳本 → 操作者在 macOS 跑 → golden 結果帶回來實作。每項改動必須同步更新 `docs/reverse-engineering/drift.md`（CLAUDE.md 規範）。

依 ROI 排序：

1. **#10 Time 邊界**（成本最低、腳本模式已存在）
   - 仿 `scripts/calibrate-structure.py` 寫 `scripts/calibrate-time.py`：控制 `.openspec.yaml` `created` 日期掃描 oracle，釘死 fresh/aging、aging/stale、stale/abandoned 三個邊界
   - 完成後更新 `calibration.rs::time_bucket` + 單元測試斷言精確邊界
2. **#9 Tasks 碰撞 positive 樣本**
   - 依 issue 描述合成強迫碰撞情境（pending task 引用檔案 → baseline 後外部 commit 改該檔）取得 positive golden；釘出 firing predicate 後實作 `tasks::analyze`、翻 `TASKS_DETECTION_CALIBRATED = true`
   - 若 oracle 掃遍情境仍全零：結論記入 drift.md（「偵測極可能是 dead feature」），gate 保持關閉，issue 關閉
3. ~~**#8 Symbol 縮窄過濾**~~ **已解，無需反組譯**（#119）
   - 「黑箱探測已窮盡」的結論下錯了：當時記下的決定性觀察「in isolation all tokens are
     kept, so the rule is global to the document」正是 document-global cap 的特徵，卻被
     讀成神秘的語意 predicate。實際機制是 `sample_over_cap`
   - 實測：30 個 Symbol 候選（總數未過 cap）全部保留，含當初引為謎題的 `Data`/`Model`
     組；83 個候選保留 `floor(i*83/12)` 的 12 個。Symbol 抽取從來沒有分歧
   - 教訓：把「規則是 document-global」當成線索追下去，而不是當成障礙
   - 後續（本 PR）：同輪 probe 補上 stoplist 漏收的 `JSON`，fresh scaffold 與 oracle 完全
     一致（#51）；`scripts/calibrate-anchor-budget.py` 把該模型固定成驗證契約（比對 anchor
     身分而非僅數量，含 snake/camel 交錯的抽取順序案例）
4. **golden 回歸自動化**
   - 新增 `tests/golden_regression.rs`：直接載入 `docs/reverse-engineering/golden/*.json`，餵進 scoring 函式比對（目前 golden 值是手抄進 `calibration.rs` 測試，fixture 與測試沒有機器連結）

**無 oracle 降級**：1–3 改為「保守維持現狀 + 文件明確標註已知分歧」，issues 標 `blocked: needs-oracle` 留存。

## Phase 5 — 品質、效能與改進（忠實期之後）（追蹤 [#29](https://github.com/howie/openspectra/issues/29)，子項 #11/#12）

1. **#12 批次化 `git grep`**：`Resolver::broken` 從 O(anchors) 子程序降到 O(1)（或改用已載入的 `tracked` 內容做 in-memory 比對）；回歸測試：40+ anchor 的合成 design.md，批次前後 `broken_anchors` byte-identical。**排在 #8 之後或確認獨立**（issue 中已記載兩者共處 resolver，需避免混淆歸因）
2. **#11 CliFlag 決策**（待人類裁決，尚未選定方向）：選項 (a) 預設忠實（永遠 broken），`.spectra.yaml` 新增 opt-in 設定；(b) 只從 fenced code block 抽 flag；(c) 指定目標 CLI 的 `--help` 做真實比對。決策記入 drift.md 的 decision note 後才開始實作與測試
3. **`drift` 建議文字 UX**：`/spectra-apply`/`/spectra-ingest` 是 Claude Code slash-command skill，非 `spectra` 二進位子指令（issue #7 已用 oracle 證據確認並關閉）；現有建議文字若措辭不夠清楚可改寫，`unarchive`（如有需求）另計
4. **改進項統一原則**：任何偏離 oracle 的行為改進一律 opt-in + CHANGELOG + drift.md「Deliberate divergences」章節記錄

---

## 測試策略總表

| 層級 | 現況 | 計畫 |
|---|---|---|
| 單元測試 | 131 個（Linux）/ 129 個（macOS），模組內 `#[cfg(test)]` | 維持慣例；root 環境 3 個誤判修掉（Phase 1） |
| 整合測試 | 1 個（synthetic repo drift） | 加 init 全流程、OpenSpec fixture、golden 回歸（Phase 1/2/4） |
| Golden 校準 | 4 個 JSON，值手抄進測試 | fixture 機器可讀化 + calibrate-time/tasks 腳本（Phase 4） |
| CI | ubuntu build+test；fmt/clippy 僅提示 | fmt/clippy 硬門檻、+macOS matrix（Phase 1）；release workflow + 容器 smoke（Phase 3） |
| E2E | 手動 | 每 phase 收尾跑 `./target/release/spectra` 真實專案全流程 |

## 建議執行順序與依賴

```
Phase 1（獨立，先做）──┬──> Phase 2（相容性，依賴 init）──> Phase 3（發佈）
                        └──> Phase 4（校準，依 oracle 可用性平行進行）──> Phase 5（改進）
```

Phase 1+2 完成即可發 `v0.1.0`（可用、可 adopt OpenSpec 專案）；Phase 4 的校準結果進 `v0.2.x`；Phase 5 的 opt-in 改進進 `v0.3.x`。

## Issue 追蹤（GitHub Issues ↔ 本文件）

每個 Phase 有一個 epic tracking issue，與本文件雙向連結；後續進度以 GitHub Issues 為準。

| Phase | Tracking issue | 狀態 | 子項 issues |
|---|---|---|---|
| Phase 1 — 基礎修補與可用性 | [#30](https://github.com/howie/openspectra/issues/30) | ✅ 完成（PR #23） | — |
| Phase 2 — OpenSpec 生態相容性 | [#26](https://github.com/howie/openspectra/issues/26) | ✅ 完成（PR #32） | 格式差異、`init --adopt`、archive MODIFIED/REMOVED/RENAMED delta |
| Phase 3 — Linux 發佈工程 | [#27](https://github.com/howie/openspectra/issues/27) | ✅ 完成（v0.2.1） | release workflow、crates.io publish、Docker/GHCR image |
| Phase 4 — Oracle 校準收尾 | [#28](https://github.com/howie/openspectra/issues/28) | open | #10 Time 邊界、#9 Tasks 碰撞、~~#8 Symbol 過濾~~（已解）、golden 回歸自動化 |
| Phase 5 — 品質/效能/改進 | [#29](https://github.com/howie/openspectra/issues/29) | open | #12 批次 git grep、#11 CliFlag 決策 |

（#7 已關閉、確認 apply/ingest 為 slash-command skill 而非 CLI 缺口，無需重開；#8–#12 沿用既有 issues，並在各 issue 留言連回其 Phase epic。）
