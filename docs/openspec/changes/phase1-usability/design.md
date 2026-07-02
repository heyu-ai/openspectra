# Design：phase1-usability

> 技術設計已定案；US-001（project-init）已實作完成，以下記錄的是實際落地的介面。

## US-001 — `init` 模組（`crates/spectra-core/src/init.rs`，已實作）

### 模組介面

```rust
pub fn init(root: &Path) -> Result<InitOutcome>

pub struct InitOutcome {
    pub root: PathBuf,          // 專案根目錄
    pub spec_dir: String,       // 固定 "openspec"（config::DEFAULT_SPEC_DIR）
    pub gitignore_updated: bool // .gitignore 是否被建立或 append
}

const GITIGNORE_ENTRY: &str = ".spectra/";
```

### 行為要點

- 進入點先檢查 `Config::is_initialized(root)`（即 `.spectra.yaml` 存在），已初始化則 `bail!("already initialized (...)")`——冪等性以「明確拒絕」表達，不做 silent re-init
- `.gitignore` 三情境：檔案不存在→建立；已有 `.spectra/` 行（trim 比對）→不動、`gitignore_updated = false`；既有內容無 trailing newline→補 `\n` 再 append
- `.spectra/` 必須進 `.gitignore` 的原因：該目錄存放 per-change sidecar state（baseline SHA、parked 標記、touched-file 追蹤），不可 commit——PR #19 的 self-recording bug 根因即是專案沒 init、沒有這條 ignore
- **未經 oracle 驗證**（假設 A1）：檔案集與訊息措辭依 README、各指令錯誤訊息（`Run 'spectra init' first.`）合理設計，需在 `docs/reverse-engineering/init.md` 標註，Phase 4 oracle 可用時回頭比對

### CLI wiring（`crates/spectra-cli/src/main.rs`，已實作）

- `Command::Init { json: bool }`：唯一**不走** `require_initialized` 的指令
- `init_json()` shape helper：`{root, spec_dir, gitignore_updated}`，仿既有 `park_status_json`/`new_change_json` pattern（shape 抽 helper + 測試釘死，typo 不 silent ship）
- `root` 以 `to_string_lossy` 序列化（非 UTF-8 path 不 panic，同 `new_change_json` 的處理）

## US-002 — `list --changes` 接線（已實作）

**檔案**：`crates/spectra-cli/src/main.rs`

- `Command::List` 的 `changes` 欄位加 `conflicts_with_all = ["specs", "parked"]`；doc comment 由 `(not yet implemented) Filter to changes only.` 改為描述「顯式版的 default 行為」
- `run()` 的 match arm 顯式 destructure `changes` 傳入 `cmd_list`；因 `--changes` 語意即 default（clap 已擋掉衝突組合），`cmd_list` 內部行為零改動——共用同一 code path 是刻意設計，保證 AC-002-1 的 byte-相容
- 測試：`Cli::try_parse_from` 斷言衝突組合 `is_err()`、合法組合 `changes == true`

## US-003 — root-skip helper（已實作）

**檔案**：`crates/spectra-core/src/touched.rs`（1 處）、`crates/spectra-core/src/archive.rs`（2 處，`#[cfg(test)]` 模組內）

```rust
/// After chmod(0o000), root (or CAP_DAC_OVERRIDE) can still read the file,
/// so the permission-denied scenario this test needs is unconstructible.
fn permission_denied_is_constructible(path: &Path) -> bool {
    std::fs::read(path).is_err()
}
```

- 每個測試在 `set_permissions(0o000)` 之後、斷言之前插入：

```rust
if !permission_denied_is_constructible(&path) {
    eprintln!("skipping <測試名>: running as root (chmod 0o000 not enforced)");
    return;
}
```

- **選 read-probe 而非 euid 檢查**：不需新增 `libc` dependency，且同時涵蓋 CAP_DAC_OVERRIDE 容器情境（euid != 0 但權限仍不 enforce）
- helper 兩份小重複分住 touched.rs / archive.rs 測試模組（repo 慣例：測試住模組內，不為測試 helper 開共用模組）
- `archive.rs:1083`（`archive_preserves_the_underlying_error_cause_after_a_post_rename_failure`）chmod 的是 `.openspec.yaml`，探測同一檔案即可

## US-004 — `ci.yml` 目標結構（已實作）

**檔案**：`.github/workflows/ci.yml`、`CLAUDE.md`

```yaml
jobs:
  lint:                      # ubuntu-only，fmt/clippy 硬門檻
    runs-on: ubuntu-latest
    steps:
      - checkout / rust-toolchain (components: rustfmt, clippy) / rust-cache
      - run: cargo fmt --all -- --check          # 無 continue-on-error
      - run: cargo clippy --all-targets -- -D warnings

  build-and-test:            # 雙平台 matrix
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - checkout / rust-toolchain / rust-cache
      - run: cargo build --release --locked
      - run: cargo test --all
      - smoke: ./target/release/spectra --help && ./target/release/spectra drift --help
```

- 前置：`cargo fmt --all` 修掉 pre-existing diff（`anchors.rs`、`calibration.rs`、`config.rs`、`drift.rs`、`tests/drift_integration.rs`）；`calibration.rs:191` 行尾註解 rustfmt 會推到極右，可讀性差就先改獨立行註解再 fmt；fmt 後立即重跑全測試證明零行為變化
- 刪除 ci.yml 的「Style checks are advisory…」過時註解
- CLAUDE.md Build/verify 一節同步：移除「`fmt`/`clippy` are `continue-on-error` in CI」描述，改為 CI 硬門檻
- macOS 測試數 -2（`#[cfg(target_os = "linux")]`）為預期，不需處理

## 衝突偵測

`docs/openspec/specs/` 不存在——本 change 是第一份 spec，**baseline，跳過衝突檢查**。
