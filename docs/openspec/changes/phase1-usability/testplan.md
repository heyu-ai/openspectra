# Testplan：phase1-usability

> 版本：v1.0 | 日期：2026-07-02 | Effort：medium
> 產出：sdd:qa-test-designer（六技法：EP / BVA / DT / ST / PW / RB）

## 1. Test Case Table

### capability: project-init（US-001，已實作）

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result |
|-------|-------------|-----------|------|-------------|-------|-----------|----------------|
| INIT-VL-001 | init 建立正確 scaffold，不覆蓋既有檔案 | VL | High (I5×L2) | 空目錄，無 `.spectra.yaml` | Rust `init::tests::init_creates_config_and_scaffold_dirs` | tmpdir，無既有檔 | 產生 `.spectra.yaml` 內含 `spec_dir: openspec`；存在 `openspec/changes/`、`openspec/specs/`；既有無關檔案內容不變 |
| INIT-VL-002 | 無 `.gitignore` 時建立並寫入 `.spectra/` | VL | Medium (I3×L3) | tmpdir 無 `.gitignore` | Rust `init::tests::init_creates_gitignore_with_spectra_entry_when_missing` | 無 `.gitignore` | 新建 `.gitignore` 含單行 `.spectra/`；JSON `gitignore_updated=true` |
| INIT-BVA-001 | 尾端無 newline 的 `.gitignore` append 前補 `\n` | BVA | High (I4×L3) | `.gitignore` 存在，最後一行無 trailing `\n` | Rust `init::tests::init_appends_to_an_existing_gitignore_without_a_trailing_newline` | 檔案內容 `"target/"`（無結尾 `\n`） | append 後為 `"target/\n.spectra/\n"`；`target/` 完整保留；`gitignore_updated=true` |
| INIT-VL-003 | 已含 `.spectra/` 不重複寫入（尾端空白視為已存在） | VL | Medium (I3×L3) | `.gitignore` 已含 entry | Rust `init::tests::init_does_not_duplicate_an_existing_spectra_gitignore_entry`（含 `.spectra/ ` 尾空白變體） | 案A `".spectra/\n"`；案B `".spectra/ \n"`（尾空白） | 兩案內容皆不變（不新增行）；`gitignore_updated=false` |
| INIT-EP-001 | 已初始化回錯且不覆蓋 | EP | High (I5×L2) | `.spectra.yaml` 已存在 | Rust `init::tests::init_errors_when_already_initialized` + integration `init_is_idempotent_refusal_not_silent_reinit` | 既有 `.spectra.yaml` | 非零 exit；錯誤含 `already initialized`；既有檔內容 byte 不變 |
| INIT-VL-004 | `--json` 輸出 shape 精確、無雜訊 | VL | Medium (I3×L3) | 空目錄 | Rust `tests::init_json_shape_matches_the_documented_contract`（spectra-cli）；解析 stdout 為 JSON | `spectra init --json` | stdout 為合法 JSON，key 恰為 `{root, spec_dir, gitignore_updated}`；`root` 為絕對路徑、`spec_dir="openspec"`、`gitignore_updated` 為 bool；無額外文字/多餘 key |
| SMK-001 | init → new change → task done → drift 全流程 | ST | High (I5×L3) | 空 git repo，已 init | Rust integration `init_then_new_change_then_drift_runs_end_to_end` + 手動 release binary 全流程（→ archive） | 單一 change，全 task 完成 | 三（四）指令皆 exit 0；drift severity = `light` |

### capability: list-changes-flag（US-002，待實作）

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result |
|-------|-------------|-----------|------|-------------|-------|-----------|----------------|
| LIST-EP-001 | `--changes` human 輸出等同 default | EP | Medium (I3×L3) | 存在 ≥1 change 的 repo | Rust 待寫：擷取 `list` 與 `list --changes` stdout 比對 | repo 有 2 個 changes | 兩者 human 文字 byte-相同；exit 0 |
| SMK-002 | `--changes --json` 與 `--json` byte 相同 | EP | Medium (I3×L3) | 存在 ≥1 change 的 repo | 手動/Rust：比對 `list --json` 與 `list --changes --json` | repo 有 changes | 兩者輸出 byte-相同的 `{"changes":[...]}`；exit 0 |
| LIST-DT-001 | `--changes --specs` clap 衝突拒絕 | DT | Medium (I3×L2) | 任意 repo | Rust 待寫：`Cli::try_parse_from(["spectra","list","--changes","--specs"])` | `list --changes --specs` | 解析 `is_err()`（clap conflict）；list 邏輯不執行 |
| LIST-DT-002 | `--changes --parked` clap 衝突拒絕 | DT | Medium (I3×L2) | 任意 repo | Rust 待寫：`Cli::try_parse_from(["spectra","list","--changes","--parked"])` | `list --changes --parked` | 解析 `is_err()`（clap conflict）；不執行 list 邏輯 |
| LIST-VL-001 | help 移除 "(not yet implemented)" | VL | Low (I2×L3) | — | 手動/Rust：擷取 `list --help` | `list --help` | 輸出不含字串 `(not yet implemented)`；exit 0 |
| LIST-VL-002 | `--changes` help 描述為顯式 default 行為 | VL | Low (I2×L2) | — | 手動檢視 `list --help` 中 `--changes` 說明 | `list --help` | `--changes` 說明文字描述其等同（顯式）default 行為，無 unimplemented 字樣 |

> DT 說明（clap 互斥矩陣）：`--changes` 與 `--specs`、`--parked` 互斥；`--changes` 單獨或不帶旗標皆為合法（等價）。impossible 組合以 clap 衝突規則保證，故無 3-flag 同開列。

### capability: root-safe-tests（US-003，待實作）

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result |
|-------|-------------|-----------|------|-------------|-------|-----------|----------------|
| ROOT-ST-001 | root/CAP_DAC_OVERRIDE 下 chmod(0o000) read 成功 → skip 並印原因 | ST | High (I4×L2) | root 或具 CAP_DAC_OVERRIDE 環境（optional：需 root 容器，無 Docker 則標註未實測） | 執行 3 個 chmod 測試，各經 `permission_denied_is_constructible(path)` 探測 | 檔案 chmod `0o000` | 探測回 false（read 成功）→ 測試判定 skip 而非 fail；stderr 印 `skipping <測試名>: running as root (chmod 0o000 not enforced)` |
| ROOT-EP-001 | 一般使用者下三測試照常執行、不誤 skip | EP | High (I4×L4) | 非 root 一般使用者（CI 主要環境） | `cargo test -p spectra-core` 執行三個既有 chmod 測試 | 檔案 chmod `0o000` | `permission_denied_is_constructible` 回 true；三測試執行原有斷言、結果不變（回歸綠）；stderr 無 skip 訊息 |
| ROOT-VL-001 | 探測機制僅用 fs::read、不查 euid、不引 libc | VL | Medium (I3×L2) | — | code review + `grep`：檢查 `permission_denied_is_constructible` 實作與 `Cargo.toml` | 原始碼 | 判定僅以 `std::fs::read(path).is_err()`；無 euid/geteuid 呼叫；依賴無新增 `libc`（涵蓋 CAP_DAC_OVERRIDE 情境） |

### capability: ci-hardening（US-004，多為 GitHub Actions 設定驗證，標 CI/manual）

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result |
|-------|-------------|-----------|------|-------------|-------|-----------|----------------|
| CI-DT-001 | fmt 為硬檢查（無 continue-on-error）、5 檔 diff 已修 | DT | High (I5×L3) | lint job 已移除 `continue-on-error`；baseline 已 `cargo fmt --all` | manual：推一筆未格式化 code 觸發 CI | 故意含格式錯誤的 commit | `cargo fmt --check` 非零；lint job 失敗；PR 合併被阻擋 |
| CI-VL-002 | baseline clean 時 fmt 過且測試數不變 | VL | Medium (I3×L3) | 已全量 `cargo fmt --all` | manual/CI：全量驗證 | 乾淨 baseline | `fmt --check` exit 0；`cargo test --all` 全綠；測試總數與修正前一致（無測試被移除） |
| CI-DT-002 | clippy `-D warnings` 為硬檢查 | DT | High (I5×L3) | lint job 含 clippy `-D warnings`、無 continue-on-error | manual：推一筆會觸發 clippy 警告的 code | 故意 unused var 等 warning | clippy 非零 exit；lint job 失敗、阻擋合併 |
| CI-PW-001 | matrix [ubuntu, macos] 測試數符合門檻 | PW | High (I4×L3) | build-and-test matrix 已設 [ubuntu, macos] | CI：讀兩 leg 的 test summary | matrix os × toolchain | ubuntu 全過（含 2 個 linux-only）；macos 全過（-2 linux-only 為預期）；非預期的測試數流失 → 判失敗 |
| CI-EP-001 | macOS 專屬編譯錯誤使整體 job 失敗 | EP | Medium (I4×L2) | matrix 含 macos leg | manual：假想情境（macOS-only 編譯錯誤） | 平台特定編譯錯 code | macos leg 失敗；matrix 使整體 job 失敗、阻擋合併 |
| CI-VL-003 | lint job 只在 ubuntu 跑一次、與 matrix 平行、macos 不重跑 fmt/clippy | VL | Medium (I3×L3) | ci.yml 定義 | CI/manual：檢視 workflow job 圖與 runs-on | ci.yml | lint job `runs-on: ubuntu-latest` 且僅一次；與 build-and-test matrix 平行；macos leg 無 fmt/clippy step |
| SMK-004 | smoke `--help` 雙平台 exit 0 | ST | Medium (I3×L3) | build 產出 release binary | CI：兩平台各跑 `./target/release/spectra --help` | 兩 leg 各一次 | 兩平台皆 exit 0；任一失敗 → 該 leg 失敗 |
| CI-VL-001 | CLAUDE.md 記載硬檢查、移除舊描述 | VL | Low (I2×L3) | CLAUDE.md 已更新 | manual：搜尋 CLAUDE.md | 文件內容 | 不含 "continue-on-error in CI" 描述；明載 fmt/clippy 為硬檢查 |
| CI-VL-004 | ci.yml 已更新但 CLAUDE.md 未同步 → 判不一致 | VL | Medium (I3×L2) | ci.yml 硬檢查、CLAUDE.md 仍寫舊描述 | manual：交叉比對 ci.yml 與 CLAUDE.md | 過期 CLAUDE.md | 判定不一致；PR 須補齊 CLAUDE.md 才可合併 |
| SMK-003 | PR 的 GitHub Actions 全綠 | ST | High (I4×L3) | PR 已開 | CI：觀察 checks | lint + build-and-test ×2 | 全部 checks 綠（lint + ubuntu + macos build-and-test） |

> PW 說明（CI-PW-001）：參數 os∈{ubuntu, macos}、toolchain（stable）、平台特有測試集（linux-only 2 個）。以 os 為主軸配對，確保每平台各驗一次測試數門檻；組合數少故僅列必要對。

---

## 2. Coverage Analysis

### Covered

| Scenario Slug | Corresponding TC IDs | Notes | 狀態 |
|---------------|----------------------|-------|------|
| init-scaffold | INIT-VL-001 | 含 MUST NOT 覆蓋斷言 | ✓ |
| init-gitignore-create | INIT-VL-002 | | ✓ |
| init-gitignore-append | INIT-BVA-001 | trailing-newline 邊界 | ✓ |
| init-gitignore-no-dup | INIT-VL-003 | 含尾空白 BVA 變體 | ✓ |
| init-already-initialized | INIT-EP-001 | | ✓ |
| init-json-shape | INIT-VL-004 | key 精確比對 | ✓ |
| init-e2e-pipeline | SMK-001 | | ✓ |
| changes-flag-same-as-default | LIST-EP-001 | human byte 比對 | ✓ |
| changes-json-shape | SMK-002 | JSON byte 比對 | ✓ |
| changes-conflicts-specs | LIST-DT-001 | | ✓ |
| changes-conflicts-parked | LIST-DT-002 | | ✓ |
| help-text-no-longer-marks-changes-unimplemented | LIST-VL-001 | | ✓ |
| help-text-describes-default-equivalent | LIST-VL-002 | | ✓ |
| root-skip-with-reason | ROOT-ST-001 | optional（需 root 容器） | ✓ |
| non-root-still-runs | ROOT-EP-001 | 主 CI 回歸 | ✓ |
| probe-mechanism-not-euid | ROOT-VL-001 | code/dep review | ✓ |
| fmt-hard-gate | CI-DT-001 | | ✓ |
| fmt-hard-gate-baseline-clean | CI-VL-002 | | ✓ |
| clippy-hard-gate | CI-DT-002 | | ✓ |
| macos-matrix | CI-PW-001 | 測試數門檻 | ✓ |
| macos-matrix-build-failure | CI-EP-001 | | ✓ |
| lint-ubuntu-only | CI-VL-003 | | ✓ |
| smoke-dual-platform | SMK-004 | | ✓ |
| claude-md-consistency | CI-VL-001 | | ✓ |
| claude-md-consistency-stale-doc | CI-VL-004 | | ✓ |

### Partially Covered

| Scenario Slug | Missing Aspect | Recommended Addition | 狀態 |
|---------------|----------------|----------------------|------|
| root-skip-with-reason | 需 root 容器；一般 CI（含本機無 Docker）無法真正執行 skip 分支，僅能靠 ROOT-EP-001 反向保證 | 若有 CI runner 具 root，補一條容器化 job 實跑 ROOT-ST-001 驗證 stderr 訊息字面 | △ |
| macos-matrix | 「131/129」為浮動門檻，硬編數字易隨新增測試過時 | 以「ubuntu 測試數 = macos + 2」相對關係理解，不硬編絕對數字 | △ |

### Completely Missing

（無——所有 scenario 皆已對應至少一個 TC）

### Redundant Items

| TC-ID | Duplicate of which TC | Recommended Action | 狀態 |
|-------|----------------------|--------------------|------|
| SMK-002 / SMK-003 | SMK-002 與 LIST-EP-001 同屬「flag 等價」但驗 JSON 面向；SMK-003 與 CI-PW-001/CI-DT-001/-002 概念重疊（皆為 CI 全綠） | 保留：SMK-002 專驗 JSON byte 相同、LIST-EP-001 驗 human；SMK-003 為端到端冒煙傘，涵蓋範圍與各 CI-* 單點不同，不移除 | — 非真冗餘 |

---

## 3. 技法選用說明

- **VL（Validation/直驗）**：對「檔案內容、JSON shape、文件字串、依賴清單」等可直接斷言的靜態產物用直驗，佔 init 與 CI 文件類多數。
- **BVA**：`.gitignore` append 的 trailing-newline 屬經典邊界（有/無結尾 `\n`），單獨立 INIT-BVA-001；no-dup 的「尾端空白 entry」併入 INIT-VL-003 作邊界變體。
- **EP**：flag 等價（`--changes` vs default）、already-initialized 錯誤類、非 root 正常路徑，皆以代表值分類；macOS build-failure 為錯誤等價類。
- **DT**：clap 旗標互斥（`--changes` × `--specs`/`--parked`）與 CI 硬檢查閘門（fmt/clippy 過/不過）以決策表列合法規則欄，impossible 組合由 clap 衝突規則排除故不展開。
- **ST**：具生命週期流程者——init→new→done→drift→archive（SMK-001）、root 探測的 skip/run 狀態切換（ROOT-ST-001）、CI 各 leg 綠/紅收斂（SMK-003/SMK-004）。
- **PW**：matrix os × 平台特有測試集，組合數少但以配對確保每平台各驗一次（CI-PW-001）。
- **RB**：以 Impact×Likelihood 定深度——init 覆蓋既有檔（I5）、CI 硬閘（I5）、root 回歸（I4×L4）列 High 做多技法覆蓋；help 文字、CLAUDE.md 純文件列 Low 僅直驗冒煙。medium effort 下六技法齊備，聚焦 happy path 與 clap 衝突、已初始化、root、CI 失敗等關鍵 error path，未對非主要欄位做完整 boundary sweep。
