# OpenSpectra 導入計畫：從 Spectra.app 遷移到各專案

> 狀態：**執行中**（2026-09-28 起草；D1–D6 已由 howie 於同日裁決，見「裁決紀錄」；
> 工作依「執行佇列」一節逐項進行）。
> 本文的「實測」都有附重現指令（見附錄）；標「推論」的是尚未驗證的判斷。

## 背景與目標

各專案目前透過 `~/.local/bin/spectra` 這個 symlink 使用 closed-source 的
`/Applications/Spectra.app/Contents/MacOS/spectra`（實測 `spectra 3.0.0 (Apple Silicon)`）。
本計畫要做到四件事：

1. **換掉**：各專案日常使用的 `spectra` 改由 OpenSpectra 提供，而且過程可隨時回退。
2. **不中斷**：不讓 Claude Code skill、yibi-stack plugin、CI 在切換當下壞掉。
3. **持續監控**：切換後，OpenSpectra 的錯誤與行為分歧要能被主動發現，不能等到使用者撞到才知道。
4. **跟上上游**：持續追蹤 oracle（`kaochenlong/spectra-app`）與 OpenSpec（`Fission-AI/OpenSpec`）的差異，在兩者之間盡量保持相容。

### 總目標與驗收條件（howie，2026-09-28）

**目標**：在 macOS 與 Linux 上都能用 OpenSpectra 取代 spectra，並相容 OpenSpec 格式。

**驗收條件**：在 macOS 上安裝 OpenSpectra release、放到 `PATH` 取代 spectra 之後：

- **A1 指令完整**：oracle 3.0.0 `--help` 列出的每個子指令與 flag，OpenSpectra 都能接受，不會出現
  `unexpected argument` 或 `unrecognized subcommand`。量測方式：`--help` flag 差集為空
  （D5 那四項延到最後做，但仍在驗收範圍內）。
- **A2 skill 能用**：3.0.0 skill 模板裡每一個 `spectra ...` 呼叫都能被 OpenSpectra 解析。
  量測方式：`cargo test` 內的模板 CLI 解析測試，CI 在 Linux 與 macOS 上都跑。
- **A3 輸出正確**：在 corpus 專案（yibi-mvp、nextrek-cli、yibi-stack 的副本）上，唯讀指令的
  exit code 與 JSON 輸出和 oracle 語意一致（validate 例外，見 A4）。會寫檔的指令
  （`new`、`task`、`archive`、`update`、`park`）在 sandbox 產生的檔案與 oracle 一致。
  量測方式：`scripts/parity-probe.py` exit 0。刻意分歧必須登錄在 probe 的允許清單並寫進 CHANGELOG。
- **A4 OpenSpec 相容**：`validate` 的判定與 `@fission-ai/openspec` 1.13.2 逐項一致；
  `--format openspec` 的 JSON 與 OpenSpec 逐欄位一致；預設的 oracle 格式在欄位上與 oracle 一致。
- **A5 Linux**：CI 的 ubuntu job 跑 A2 與無 oracle 版本的整合測試全綠；release 有 Linux musl 產物。

**不在範圍內**：刪除 Spectra.app。它是本 repo 所有 `scripts/capture-*.py` 校準腳本的 oracle，
「解除」指的是把它移出各專案的 `PATH`，不是從機器上移除（見 Phase 5）。

## 現況盤點（2026-09-28 實測）

### 安裝面

| 項目 | 實測 |
|---|---|
| `spectra` | `~/.local/bin/spectra` → `/Applications/Spectra.app/Contents/MacOS/spectra`，3.0.0；`PATH` 裡 `~/.local/bin` 出現兩次，所以 `which -a` 會列出兩筆 |
| `openspec` | npm global `@fission-ai/openspec@1.5.0`，裝在 `~/.hermes/node/bin/`，**不在 `PATH` 上** |
| 上游最新版 | oracle v3.0.0（2026-09-11）；OpenSpec npm `1.13.2`（2026-09-23）；本 repo 的 issue 目前追到 1.13.1（#188／#189） |
| OpenSpectra | 最新 release `v0.13.0`（2026-09-27） |
| 權限 | `~/.claude/settings.json` 有 `Bash(spectra:*)`，換成 OpenSpectra 後仍然適用，因為 binary 名稱相同 |

### 使用 spectra 的專案（路徑相對 `~/Workspace/github/`）

| 專案 | 特徵 | 生成的 skill 數 | 風險 |
|---|---|---|---|
| `heyu-ai/yibi-mvp` | 22 個 active change、80 份 spec、自訂 schema `spec-driven-yibi`、`locale: tw`、`.spectra/spectra.db`；CI 已混用 openspec 1.5.0 與 OpenSpectra v0.8.0（`docs/openspec/spectra-vendor/`，ADR-0021） | 13 | 高 |
| `heyu-ai/yibi-mvp-fix-1826/1827/1828/1833` | yibi-mvp 的 linked worktree（`--git-common-dir` 指向 `yibi-mvp/.git`），對應的 PR #2011／#2007／#2008／#2010 都已 merge | 13 | 不遷移，清掉（D4） |
| `yibi-stack` | plugin 原始碼 repo；root `openspec/` 有 8 個 active change，`docs/openspec/` 另有 3 個 | 13 | 高（它同時是 skill 的上游） |
| `heyu-ai/nextrek-cli` | `worktree: true`、多數 skill 的 `claude_effort` 設為 `xhigh` | 13 | 中 |
| `heyu-ai/yibi-agent` | 3 個 active change | 12 | 中 |
| `openab-projects/openab-console` | 只有 config | 12 | 低 |
| `heyu-ai/storysonic-lab`、`heyu-ai/yibi-stackchan` | changes 只剩 archive | 12 | 低 |
| `side-project/MiniShell` | `docs/openspec/` 是空的 | 9 | 低（首選試點） |
| `ainization-skill` | `docs/openspec/changes/` 是空的 | — | 低 |
| `doxa/coach_service/coachly` | 有 `.spectra/spectra.db` | — | 中（內容未盤點） |
| `openab-projects/openab-workspace` | root `openspec/` 沒有 `.spectra.yaml`，是純 OpenSpec 專案 | — | 需要 `init --adopt` |

生成的 skill 數量不一（9／12／13），代表各專案是用**不同版本的 oracle** 跑 `spectra update`
產生的 skill。這一點會直接影響下面的「消費端合約」。

### 誰在呼叫 CLI（消費端）

1. **專案內 `spectra update` 產生的 `.claude/skills/spectra-*`**：這是最大的消費端。以 yibi-mvp
   現有的 skill 統計呼叫次數：`list --json`（11 次）、`status --change`（8）、`archive`（6）、
   `analyze`（6）、`search`（5）、`unpark`／`list --parked`（各 4）、`instructions`（9）、
   `in-progress add`（3）、`drift`（3）、`new artifact`（3）、`validate`／`task done`／`new change`（各 2）。
   其中 `spectra sync`（`spectra-commit` 的 SKILL.md:136）在 oracle 3.0.0 與 OpenSpectra
   裡**都不存在**，是一個已經失效的呼叫。
2. **yibi-stack plugin**（`sdd`、`dev-cycle` 的 1.20.1／1.23.3）：主要呼叫
   `spectra archive <name> --yes`，另外有 `analyze`、`list`、`validate`、`status`、`schemas`。
3. **CI**：只有 yibi-mvp 家族。`spec-drift-backlog.yml:62` 與 `ci.yml:2152` 從
   `github.com/howie/openspectra/releases/...` 下載 `OPENSPECTRA_VERSION` 指定的版本（目前 `v0.8.0`）。
   這個舊 URL 目前還能靠 GitHub 轉址下載（實測 200），但 repo 已經搬到 `heyu-ai/`，應改成新網址。
   `ci.yml:1586-1594` 另有兩個 gate，是為了繞過 oracle archive「只掃第一個 `## Requirements`」
   的 bug（yibi-mvp #982）而寫的；對應到本 repo 的 #184。

## 相容性實測：在 yibi-mvp 副本上比對兩個 binary

做法：把 yibi-mvp 的 `.spectra.yaml`、`.claude/`、`CLAUDE.md`、`docs/openspec/` 複製到暫存目錄並
`git init`，在同一份檔案上分別跑 oracle 3.0.0 與 OpenSpectra `v0.13.0`（worktree HEAD `e663924`
的 release build），逐指令比對 exit code 與輸出。

### 結論先講：目前不是 drop-in 替代品

13 個唯讀指令裡，只有 `list --parked` 的輸出逐字相同。把 JSON 的 key 排序後再做語意比對，
差異依然存在，而且其中幾項會讓現有消費端壞掉。可以歸成三個**阻擋項**：

**阻擋項 B1：OpenSpectra 自己的 `update` 產生的 skill，會呼叫 OpenSpectra 不支援的指令與 flag。**
`update` 的模板是逐位元組照 oracle 3.0.0 抓的（`docs/reverse-engineering/update.md`），但 3.0.0
模板用到的 CLI 介面，OpenSpectra 還沒有移植。clap 碰到這些呼叫會直接報
`error: unexpected argument`（實測 `instructions apply --compact`），不會忽略。

| 模板裡的呼叫（6 家 AI 工具合計次數） | OpenSpectra 狀態 | 追蹤 |
|---|---|---|
| `instructions <artifact> ... --omit-context`（11） | 缺 flag | **無 issue** |
| `instructions apply ... --compact`（7）、`--summary`（6） | 缺 flag | **無 issue** |
| `instructions --skill <name> --agent`（11） | 缺 `--agent` | **無 issue** |
| `instructions proposal ... --type`（6） | 缺 flag | **無 issue** |
| `new change ... --agent`（12）、`--schema no-spec`（6） | 缺 flag，也沒有 `no-spec` schema | **無 issue** |
| `task done ... --file`（6）、`task start`（6） | 缺 | #190 |
| `archive <name> --preview`（3） | 缺 `--preview`、`--json` | **無 issue** |
| `scope`（26） | 缺指令 | #165 |

所以在 B1 解決之前，**任何專案都不能在切換後跑 `spectra update`**；否則 skill 會被換成 3.0.0 版，
而 3.0.0 版的 skill 呼叫的東西 OpenSpectra 做不到。

**阻擋項 B2：JSON 輸出的形狀不同。** skill 與 plugin 會解析這些欄位：

| 指令 | 差異（oracle → OpenSpectra） |
|---|---|
| `list --json` | 少了 `changes[].summary`；同分時排序不同 |
| `list --specs --json` | 欄位名稱不同：oracle 是 `id`／`path`，OpenSpectra 是 `name`／`summary` |
| `show <change> --json` | 少了 `created`、`deltaSpecs`、`design`、`schema`、`tasks` |
| `validate --json` | 整體形狀不同：oracle 回傳 `[{change, valid, errors, warnings}]` 陣列，OpenSpectra 回傳 OpenSpec v2 的 `{items: [...]}` |
| `drift --json`、`instructions --json` | 少了 3.0.0 新增的 `dormancy.*`、`recommended_action.action_kind` 等欄位 |
| `instructions apply --json` | 少了 `tasks[].cycle_member`、`mixed_format` |
| `status --change --json` | artifact 順序不同（第 2、3 個 artifact 的 `design` 與 `specs` 對調）；OpenSpectra 多了 `requires`、`isPlanningComplete` |
| `schemas --json` | oracle 列 3 個 schema（多了 `no-spec`），描述文字也不同 |

**阻擋項 B3：判定結果不同。**

- `validate --changes`：oracle 判 **0/21** 無效（exit 0），OpenSpec 1.13.2 判 **8/21**，
  OpenSpectra 判 **9/21**（exit 1）。3.0.0 skill 在 archive 前會跑 `spectra validate "<name>"`
  （看 exit code 與文字，不帶 `--json`），所以切換後會卡在這些 change。詳見下方三方比對。
- `instructions`：OpenSpectra 有讀 `.spectra.yaml` 的 `locale: tw`（`config.rs:14`），但輸出的
  `locale` 仍是 `English`，oracle 則是 `Traditional Chinese (繁體中文)`。這會讓 agent 用錯語言寫 artifact。
- `analyze`：dimension 數量不同（oracle 5 個，OpenSpectra 4 個），各 dimension 的 finding 數量也不同（#169）。

### validate 嚴格度：oracle、OpenSpec、OpenSpectra 三方比對

同一份 yibi-mvp 副本、同樣 21 個 change，分別用三個工具跑 `validate --changes --json`
（OpenSpec 用 `npx @fission-ai/openspec@1.13.2`，在副本加一個 `openspec -> docs/openspec`
symlink，因為它固定從 cwd 找 `openspec/`）：

| 規則 | oracle 3.0.0 | OpenSpec 1.13.2（原始碼位置） | OpenSpectra v0.13.0 | 受影響的 change |
|---|---|---|---|---|
| MODIFIED 區塊漏掉主 spec 仍有的 scenario | 不檢查 | **ERROR**（`dist/core/validation/validator.js:645`，#1477；理由是 archive 會整塊取代，漏掉就等於刪掉） | ERROR，但**同一件事報兩則**（#183） | 0070、0098、0140、0141、0148 |
| change 完全沒有 delta | WARNING「No delta specs found」，仍判 valid | **ERROR**（`validator.js:448`，除非 `.openspec.yaml` 宣告 `skip_specs`） | ERROR | 0084、0126、0144 |
| ADDED／MODIFIED requirement 沒有 scenario | 不檢查 | ERROR（`validator.js:257`、`:289`） | ERROR，另外在 0098 多報了一則 OpenSpec 沒報的 | 0098 |
| MODIFIED 指向主 spec 不存在的 requirement | WARNING（archive 會拒絕） | **INFO**「Archive would refuse this delta」（`validator.js:842`），仍判 valid | **ERROR** | 0046 |
| 目標 requirement 帶有無法辨識的 `@trace` footer | 無 | 無 | ERROR（OpenSpectra 獨有，來自 sidecar 分歧 #179） | 0141 |
| **合計無效** | **0** | **8** | **9** | |

解讀：

- oracle 的 validate 對這些語意問題全部放行，問題要到 archive 時才浮現：0046 的 MODIFIED
  目標不存在，oracle 自己的 warning 就說 archive 會拒絕。0070 等 5 個漏 scenario 的 change，
  MODIFIED 是整塊取代，所以推論 oracle archive 後那些 scenario 會消失（**未實測**，要在 M4
  用 oracle 跑一次 archive 確認）。OpenSpec 則是把這類「archive 會出事」的情況提前到 validate 擋下。
- OpenSpectra 大致跟隨 OpenSpec，但有三處偏離：比 OpenSpec 嚴（0046 的 ERROR 應為 INFO）、
  重複回報（#183）、0098 多一則誤報（待查），以及獨有的 trace footer 檢查。
- 依 D1 裁決（先與 OpenSpec 一致），上面三處偏離要修掉；trace footer 那條屬於 OpenSpectra
  的刻意分歧，要另外決定在「OpenSpec 模式」下是 ERROR 還是降級（見 M4）。
- 對 yibi-mvp 的實際意義：切換後有 8 個 change 需要修。其中漏 scenario 的 5 個與沒有 delta 的
  3 個，在 OpenSpec 的規則下本來就不合格，所以這主要是把既有問題提早揭露。

### `update` 的副作用（與相容性無關，但切換時一定會遇到）

在副本上跑 OpenSpectra `update` 之後：

- 11 個 skill 被改寫、新增 `spectra-review`（+908／−1555 行）。
- **`CLAUDE.md` 的 `<!-- SPECTRA:START -->` 區塊整段被覆寫**（v1.0.2 → v1.3.0），yibi-mvp 手動加在
  區塊內的「PR #1456 教訓」那一行因此被刪除。
- `.claude/settings.json` 結尾的換行被拿掉。

這些都是 oracle 本來就有的行為（模板逐位元組對齊），不是 OpenSpectra 的 bug。但遷移 runbook 必須
先把手寫內容搬出受管區塊，並且在 commit 前審 diff。

### 兩邊的 flag 差集（`--help` 掃描）

| 子指令 | 只有 oracle 有 | 只有 OpenSpectra 有 |
|---|---|---|
| `show` | `--deltas-only`、`--requirements`、`--item-type` | `--diff` |
| `instructions` | `--agent`、`--compact`、`--omit-context`、`--summary`、`--type` | — |
| `new change` | `--agent`、`--description`、`--schema` | `--json` |
| `archive` | `--preview`、`--json` | — |
| `task done` | `--file` | — |
| `validate` | — | `--strict`、`--type`、`--archived`、`--report` |
| `status` | — | `--all` |
| `init` | — | `--adopt`、`--json` |
| 整個指令 | `scope`、`decisions`、`task start`、`feedback`、`demo` | `search`、`trace migrate` |

OpenSpectra 多出來的部分（`validate --strict`、`search`、`trace`）是刻意的擴充，不構成相容性問題。
但要注意：舊版 oracle 產生的 skill 會呼叫 `spectra search`，**oracle 3.0.0 已經移除這個指令**，
OpenSpectra 反而還留著。所以對舊 skill 來說，OpenSpectra 在這一點上的相容性比 oracle 3.0.0 還好。

## 遷移原則

1. **相容目標是「消費端合約」，不是完整重現 oracle。** 合約指的是：現有 skill、plugin、CI 實際
   呼叫的指令、flag、exit code，以及它們會讀的 JSON 欄位。合約用 parity probe 的結果定義，
   而且要納入版本控管（見 Phase 0 M1）。合約以外的差異，照本 repo 既有原則處理
   （忠實優先；刻意分歧要 opt-in、寫進 CHANGELOG）。
2. **相容性的優先序：消費端合約 > oracle 3.0.0 > OpenSpec。** 三者衝突時先保住合約；oracle 與
   OpenSpec 互相矛盾時（#189），由人裁決，不由 agent 自行決定（見 `CLAUDE.md` 的 Agent conduct）。
3. **切換必須可以一鍵回退。** 由 OpenSpectra 內建的實作切換（`OPENSPECTRA_IMPL`）決定實際執行者，
   不靠逐台機器改 symlink（見 Phase 1）。
4. **先 shadow 再切換。** 先讓 OpenSpectra 在背景跑唯讀指令並記錄差異，差異收斂之後才讓它成為主要實作。
5. **由低風險到高風險，逐個專案切換**，每個專案切換後至少觀察一週再換下一批。

## 執行佇列

以 loop 逐項執行，一項一個 PR（從 `origin/main` 開分支，彼此獨立）。狀態欄在每項開 PR 時更新。

| # | 工作項 | 對應驗收 | 追蹤 | 狀態 |
|---|---|---|---|---|
| W1 | 量測工具：模板 CLI 解析測試（`cargo test`，Linux／macOS 皆跑）＋ `scripts/parity-probe.py`（oracle 比對、允許清單） | A2、A3 的量尺 | — | PR #196（A2 缺口 41 條、A1 缺口 19 項、A3 已知分歧 124 條） |
| W2 | `instructions` 補 `--omit-context`／`--compact`／`--summary`／`--agent`／`--type` | A1、A2 | — | PR #197（stacked on #196；另補 5 個漏抓的 skill、`contextRef`、spec_dir 代入；驗收矩陣 204/208 相同） |
| W3 | `new change` 補 `--agent`／`--description`／`--schema`，內建 `no-spec` schema；`schemas`／`status` 的 artifact 順序對齊 3.0.0 | A1、A2、A3 | — | PR #198（stacked on #197；`status` 順序實測本來就一致；新增 `capture-schemas.py`；parity 119 → 110） |
| W4 | `task start`、`task done --file`（per-task baseline） | A1、A2 | #190 | PR #201（stacked on #199；golden replay 擴大到 14 情境、360 欄位全部逐位元組相同；#98 移除；parity 108 → 106） |
| W5 | `archive --preview`／`--json` | A1、A2 | — | PR #199（stacked on #198；preview 10 情境逐位元組相同；`snapshot_created: false` 為刻意分歧，對應 #111；parity 110 → 108） |
| W6 | `scope`（含 `--change`／`--base`／`--check-snapshot`／`--json`） | A1、A2 | #165 | **待裁決| W7a | `list`（`summary`、排序、名稱過濾）、`list --specs`、`show`、`schema which` 對齊 3.0.0 | A3 | — | PR #202（stacked on #201；29 項 oracle 比對全部相同；parity 106 → 97） |
| W7b | `drift`／`instructions apply` 的 `dormancy`、`recommended_action`、apply `tasks[]` 的 `number`／`prerequisites`（`[after: …]`）／`unresolved_prerequisites`／`cycle_member`／`mixed_format`／`parallel` 規則 | A3 | — | 待辦（RE 規格已完成） |
| W7c | 內建 spec-driven schema 的 instruction／template 文字對齊 3.0.0（`capture-schemas.py` 加入 spec-driven） | A3 | — | 待辦 |cenario subject rule 兩段） | A3 | 新 issue | 待辦 |
| W8 | `locale` 對應（`tw` 等）套用到 `instructions` 等輸出 | A3 | 新 issue | 待辦 |
| W9 | `validate`：規則對齊 OpenSpec 1.13.2（含 #183）＋ `--format oracle`（預設）／`openspec` | A1、A4 | #189、#183 | 待辦 |
| W10 | `analyze` 對齊 3.0.0（dimension 數與檢查項） | A3 | #169 | 待辦 |
| W11 | 內建實作切換 `OPENSPECTRA_IMPL`／`.spectra/impl`／shadow／`spectra impl` | 切換與回退 | 新 issue | 待辦 |
| W12 | D5 項目：`decisions`、`show --deltas-only/--requirements/--item-type`、`demo`、`feedback` | A1 | #166、#62 | 待辦（最後做） |
| W14 | `init` 對齊 3.0.0（oracle 預設寫出 `spec_dir: docs/spectra`，OpenSpectra 仍是 `openspec`）；未初始化錯誤訊息改為 oracle 的 `Not initialized. Run 'spectra init' to initialize.` | A3 | W2 發現 | 待辦（W13 之前做） |
| W13 | 總驗收：本機整合分支合併所有 W 分支，release build 安裝到 `~/.local/bin`，在 corpus 上跑 A1–A4，列出剩餘問題並回填佇列 | A1–A5 | — | 待辦 |

**每一項的標準流程**：

1. 從 `origin/main` 建 worktree 與分支；`git branch --show-current` 確認分支。
2. 先在 sandbox 用 oracle 探測行為並記下 golden（一個 jail 只做一個操作），寫進對應的
   `docs/reverse-engineering/*.md`；oracle 與 OpenSpec 衝突時停下來回報，不自行裁決。
3. 先寫會失敗的測試，再實作（TDD）；新行為放 `spectra-core`，CLI 只做接線。
4. 跑 `cargo fmt --all -- --check`、`cargo clippy --all-targets -- -D warnings`、
   `cargo build --release --locked`、`cargo test --all`，全綠才推送。
5. 更新 CHANGELOG `[Unreleased]`、README（如有新指令），用 explicit refspec 推送並開 PR，
   PR body 寫 `Closes #N`（只列真的要關的）。
6. **PR 不由 agent merge**，由 howie 審查後合併。W13 的整合驗收在本機整合分支上進行，不需要等 merge。

## 分階段計畫

```
Phase 0（openspectra repo：補阻擋項 + parity 基礎建設）
   └─> Phase 1（內建實作切換 + shadow 模式）
          └─> Phase 2（試點：低風險專案）
                 └─> Phase 3（中風險專案）
                        └─> Phase 4（yibi-mvp、yibi-stack、CI）
                               └─> Phase 5（Spectra.app 移出 PATH）
監控與上游追蹤（貫穿 Phase 1 之後的所有階段）
```

### Phase 0 — 在 openspectra repo 補齊阻擋項

**M1. 把 parity probe 產品化成驗證契約。** 附錄 A 的一次性腳本要改寫成
`scripts/parity-probe.py`（macOS + oracle，性質同其他 `capture-*` 腳本），規格如下：

- 輸入：一個或多個 corpus 專案路徑（先複製到暫存目錄，確保不會動到原檔）。
- 對每個唯讀指令，同時比對 exit code、JSON 語意（key 排序後比較），以及「消費端合約欄位」清單。
- 合約清單以 TSV 形式放在 `docs/reverse-engineering/golden/consumer-contract.tsv`。
- 合約欄位出現分歧時 exit 非 0，並保留失敗的 sandbox 供檢查（依 `CLAUDE.md`：calibration
  腳本是驗證契約，不是印表機）。
- 另外掃描 `update` 模板裡所有的 `spectra ...` 呼叫，逐一確認 OpenSpectra 的 clap 能不能解析
  （`--help` 比對，或實際以 `--help` 探測各子指令的 flag）。模板用到但 CLI 不支援的，一律列為失敗。
  **這個檢查可以跑在 CI 上**（不需要 oracle），能防止 B1 再次發生。

**M2. 解決 B1**（模板呼叫的 CLI 介面）。依呼叫次數排序：
`instructions --omit-context/--compact/--summary/--agent/--type` → `new change --agent/--schema`
加上 `no-spec` schema → `task start`／`task done --file`（#190）→ `scope`（#165）→ `archive --preview/--json`。
每一項都要先抓 oracle golden（依本 repo 慣例）。依 D2 裁決採 (a)：先補齊 CLI 介面，
`update` 維持逐位元組輸出 oracle 模板，不做降級版模板。在 M2 完成之前，任何專案都不跑 `update`。

**M3. 解決 B2**：`list`／`list --specs`／`show`／`drift`／`instructions` 的 JSON 欄位補齊到 3.0.0。
`validate --json` 依 D3 裁決提供兩種格式，兩者的差異如下：

| | oracle 3.0.0 格式 | OpenSpec 格式（OpenSpectra 現行，README 稱「v2」） |
|---|---|---|
| 頂層 | 陣列 `[...]` | 物件 `{items, summary, version, root}` |
| 每個項目 | `{change 或 spec, valid, errors, warnings}`，用 key 名稱區分是 change 還是 spec | `{id, type, valid, issues, durationMs}`，用 `type` 欄位區分 |
| 問題清單 | `errors`、`warnings` 兩個**字串陣列**，只有訊息文字 | 單一 `issues` 陣列，每筆是 `{level: ERROR/WARNING/INFO, path, message, line?}`，有嚴重度、檔案路徑，部分有行號 |
| 彙總 | 無，要自己數 | `summary.totals`（passed／failed／items）與 `summary.byType` |
| 其他 | 無 | `version`、`root`（專案根目錄） |

OpenSpectra 現行輸出和 OpenSpec 1.13.2 原版只差三處：`version` 是 `"2.0"`（OpenSpec 是 `"1.0"`）、
`summary.totals` 多一個 `total`、`root` 是 `{path, spec_dir}`（OpenSpec 是 `{path, source}`）。

做法：新增 `--format oracle|openspec`，**預設 `oracle`**（D3），直到 openspec 格式與 OpenSpec 1.13.2
逐欄位一致後再重新評估預設值。openspec 格式要補齊上述三處差異。
注意：這是對現行 OpenSpectra 輸出的 breaking change（README 的 CI gate 範例、yibi-mvp 的
`openspec_validate_report.py:869` 讀 `summary.totals.failed`），CHANGELOG 要標明，
README 範例改成明確帶 `--format openspec`。

**M4. 解決 B3**：
- `validate` 依 D1 裁決先與 OpenSpec 1.13.2 一致：MODIFIED 目標不存在降為 INFO「Archive would
  refuse this delta」、scenario 遺失只報一則（#183）、查明 0098 多報的「缺 Scenario」是否為誤報。
  trace footer 檢查是 OpenSpectra 的刻意分歧，在 OpenSpec 一致性的比對中要排除或另外標註。
  驗收：在 yibi-mvp 副本上與 `openspec validate --changes --json` 的判定逐 change 相同（8/21）。
- 另外用 oracle 在副本上實際 archive 一個漏 scenario 的 change（例如 0070），確認 oracle 是否真的
  會刪掉 scenario，把結果記進 `docs/reverse-engineering/validate.md`，作為「跟 OpenSpec 而不跟
  oracle」的證據。
- `locale` 的對應（`tw` → `Traditional Chinese (繁體中文)`）、`analyze` 的 dimension（#169）。

**M5. 其他 3.0.0 缺口**：`decisions`（#166）、`show` 的 `--deltas-only`／`--requirements`／
`--item-type`、`demo`（#62）、`feedback`。實測沒有任何消費端（見「裁決紀錄」D5）。**建議**
列為最低優先，等 M1–M4 完成後再依需要移植；要不要做仍待決定。

**Phase 0 出口條件**：parity probe 在 yibi-mvp、nextrek-cli、yibi-stack 三個 corpus 上，
消費端合約欄位 0 分歧；模板 CLI 解析檢查在 CI 上全綠；發一個 release。

### Phase 1 — OpenSpectra 內建實作切換與 shadow 模式

依 D6 裁決，「由誰來執行 `spectra`」的切換機制做成 OpenSpectra 內建功能，不另外寫外部 shim script。
（shim 指的是插在呼叫端與真正程式之間的一層薄轉接：呼叫端照舊執行 `spectra`，轉接層再決定要交給
oracle 還是 OpenSpectra。內建的意思是這層轉接就寫在 OpenSpectra 的 binary 裡。）

1. **新增到 openspectra 的功能**（需要另開 issue 與 PR，屬 Phase 0 之後的第一個開發項）：
   - 實作選擇：環境變數 `OPENSPECTRA_IMPL=oracle|shadow|oss`，優先於專案層級的
     `.spectra/impl`（`.spectra/` 本來就在 `.gitignore` 裡，是本機狀態），再優先於使用者層級設定；
     都沒設定時為 `oss`。
     刻意**不**寫進 `.spectra.yaml`，因為 oracle 也會讀這個檔，不要在 oracle 的設定檔裡放它不認識的 key。
   - oracle 路徑：預設 `/Applications/Spectra.app/Contents/MacOS/spectra`，可用 `OPENSPECTRA_ORACLE_BIN`
     覆寫。選了 `oracle` 或 `shadow` 但找不到 oracle 時直接報錯並 exit 非 0，不可靜默改用 `oss`
     （Linux 上本來就沒有 oracle，所以這兩個模式只在 macOS 有意義）。
   - `oracle` 模式：原封不動地 exec oracle（argv、stdin、exit code 全部透傳）。
   - `shadow` 模式：以 oracle 的結果為準回傳給呼叫端。若是唯讀指令（`list`、`show`、`status`、
     `validate`、`analyze`、`drift`、`instructions`、`schemas`、`templates`），OpenSpectra 在同一個
     process 內也算一次，比對 exit code 與輸出（JSON 做語意比對，與 M1 共用比對邏輯），不同時寫一筆
     JSONL 到 `~/.local/state/openspectra/shadow.jsonl`（時間、cwd、argv、兩邊 exit code、差異摘要）。
     **會寫檔的指令（`archive`、`task done`、`new`、`park`、`update`……）絕對不能 shadow**，
     因為兩個實作會對同一份檔案各寫一次；這份唯讀白名單要寫死在程式碼裡並有測試守住。
   - `oss` 模式：正常執行，另外把非 0 exit、stderr 摘要、panic 記到 `errors.jsonl`。
   - 新增 `spectra impl` 子指令（OpenSpectra 獨有），顯示目前生效的模式、設定來源與 oracle 路徑，
     方便排查「現在到底是誰在跑」。
2. 安裝：把 OpenSpectra release 放到 `~/.local/bin/spectra`，取代現在指向 Spectra.app 的 symlink。
   因為預設是 `oss`，**在 shadow 驗證期間要全域設定 `OPENSPECTRA_IMPL=shadow`**，讓行為仍以 oracle 為準。
3. 本機全域開 `shadow` 一週，累積真實使用下的差異樣本。回退：`OPENSPECTRA_IMPL=oracle`，
   或把 symlink 指回 Spectra.app。

**Phase 1 出口條件**：shadow log 裡沒有未分類的消費端合約分歧。每一筆分歧都已經分類為
「開了 issue」「刻意分歧」或「不影響消費端」。

### Phase 2 — 試點（低風險）

依序：`side-project/MiniShell` → `ainization-skill` → `heyu-ai/yibi-stackchan` →
`heyu-ai/storysonic-lab` → `openab-projects/openab-console`。每個專案照下面的 runbook 做。

#### 每個專案的切換 runbook

1. **前置**：確認沒有其他 session 正在該專案跑 spectra 流程；從 `origin/main` 開分支。
2. **備份受管區塊內的手寫內容**：`CLAUDE.md`／`AGENTS.md` 在 `<!-- SPECTRA:START -->` 與
   `<!-- SPECTRA:END -->` 之間，如果有人手動加的內容，先搬到區塊外。`update` 會整段覆寫這個區塊。
3. **寫入 `.spectra/impl` 內容為 `oss`**（`.spectra/` 已被 gitignore，這是本機設定）。
4. **只讀驗證**：`spectra list --json`、`spectra validate --changes`、`spectra status --change <x> --json`，
   並對照 oracle 的輸出（`OPENSPECTRA_IMPL=oracle` 再跑一次）。
5. **`spectra update`**（只在 Phase 0 B1 解決之後）：審 diff。重點看受管區塊、`settings.json`、
   新增或刪除的 skill，確認沒有 3.0.0 已移除的 skill 殘留（#170）。
6. **trace 格式**：OpenSpectra 的 archive 會寫 `spec.trace.yaml` sidecar，oracle 則寫 inline footer
   （刻意分歧，ADR-0029 D3）。切換後跑一次 `spectra trace migrate --dry-run` 看影響範圍；
   如果同一個專案還有人或機器繼續用 oracle，要在 CI 加 `spectra trace migrate --check`。
7. **走一次完整流程**：用一個小的 change 跑 `propose → apply → archive`，確認 skill 端到端都能走通。
8. **commit、開 PR**，PR 描述附上步驟 4 與 5 的比對結果。
9. **觀察一週**，看監控（見下文）有沒有新的錯誤。

### Phase 3 — 中風險

`heyu-ai/yibi-agent`、`heyu-ai/nextrek-cli`（`worktree: true`，要特別驗證 parked change 與
worktree 的路徑）、`doxa/coach_service/coachly`（先盤點 `.spectra/spectra.db` 的用途，
OpenSpectra 不會讀這個檔）、`openab-projects/openab-workspace`（沒有 `.spectra.yaml`，
用 `spectra init --adopt` 接入；要確認不會覆蓋它的 `openspec/` 內容）。

### Phase 4 — yibi-mvp、yibi-stack 與 CI

1. **yibi-mvp CI**：
   - 下載網址從 `howie/openspectra` 改成 `heyu-ai/openspectra`。
   - `OPENSPECTRA_VERSION` 從 `v0.8.0` 升到 Phase 0 的 release。
   - 評估 `openspec-validate` job 能不能改用 `spectra validate --changes --strict`。README 說
     OpenSpectra 會走訪巢狀 `specs/<Epic>/<Feature>/spec.md`，這正是 yibi-mvp 的格式。
     改完之後就不用再裝 Node 版 openspec 1.5.0。
   - `ci.yml:1586-1594` 那兩個繞過 oracle bug 的 gate 先保留。OpenSpectra 的 archive 已經能歸屬
     任何 `##` 區段裡的 footer（#179／PR #180），但 retire 判斷仍只看第一個 `## Requirements`（#184 open）。
     要等 #184 修好，並用 yibi-mvp #982 的案例實測過，才能移除。
2. **yibi-mvp 本機**：照 runbook 操作，並加做：自訂 schema `spec-driven-yibi` 的 `schema which`／
   `schema validate`、`locale: tw` 的輸出語言、`claude_effort` 與 `parallel_tasks` 等設定值
   是否被 `update` 正確帶入 skill frontmatter。
3. **`yibi-mvp-fix-18xx`**：依 D4 清掉，不遷移（檢查結果與待確認事項見「裁決紀錄」D4）。
4. **yibi-stack plugin**：plugin 呼叫 `spectra archive <name> --yes`，兩個實作都支援。
   但 plugin 文件應註明「支援 OpenSpectra ≥ vX」，而且 plugin 自己的 CI 應該要能用 OpenSpectra
   跑它文件裡的範例指令。

### Phase 5 — 把 Spectra.app 移出 PATH

條件：所有專案都已經切到 `oss` 至少兩週，shadow／oss log 裡沒有未處理的合約分歧。

1. 移除全域的 `OPENSPECTRA_IMPL=shadow` 設定，讓預設值 `oss` 生效（仍保留用 `OPENSPECTRA_IMPL=oracle`
   手動切換的能力）。
2. Spectra.app **保留在 `/Applications`**，但關掉它的自動更新（推論：它有自己的更新機制；
   若會自動更新，就會讓 golden 在不知情的情況下換版本）。它的角色改成純粹的 oracle，
   只有校準腳本會用到。
3. 在 `~/.claude/CLAUDE.md` 或各專案的文件中，把「spectra」的說明改成指向 OpenSpectra。

## 持續監控

### 一、OpenSpectra 自己的錯誤

| 來源 | 做法 | 頻率 |
|---|---|---|
| 內建實作切換 | `oss` 模式記錄非 0 exit、stderr、panic 到 `~/.local/state/openspectra/errors.jsonl`；`shadow` 模式記錄分歧到 `shadow.jsonl` | 即時 |
| 彙整報告 | 一支 `scripts/shadow-report.py`：依指令與分歧欄位分組，找出新出現的分歧類型，產生 markdown 摘要。可以接到現有的 `call-it-a-day`／`km-daily-review` 流程裡 | 每日或每週 |
| clap 解析錯誤 | `unexpected argument` 這類錯誤代表有 skill 呼叫了 OpenSpectra 不支援的介面，要當成 P1 處理（B1 類問題又出現了） | 即時 |
| 專案 CI | yibi-mvp 的 `spec-drift-backlog.yml` 已經有 report-only 的 drift 報告；其他專案可以加 `spectra validate --changes` 與 `spectra trace migrate --check` | 每個 PR |
| 本 repo CI | M1 的「模板 CLI 解析檢查」 | 每個 PR |

### 二、與上游的差異

本 repo 已經有 `.github/workflows/upstream-watch.yml`：每週一抓 oracle 與 OpenSpec 過去 8 天的
release 與已關閉 issue，有變動就開一個 digest issue。它只回答「上游發生了什麼」，沒有回答
「我們因此落後了多少」。建議補上以下三項：

1. **版本落差檢查（可以放在 CI）**：在 `upstream-watch.yml` 加一個 step，比較
   (a) oracle 最新 release 與本 repo golden 所釘的版本（目前 `update-trees-3.0.0.tsv`、`skills-3.0.0.tsv`）；
   (b) npm 上 `@fission-ai/openspec` 的最新版與本 repo 宣稱相容的版本（README 寫 1.12，
   issue 追到 1.13.1，npm 最新是 1.13.2）。有落差就寫進 digest。
2. **oracle 升版時的重新校準（需要 macOS，所以只能在本機跑）**：Spectra.app 版本一變，就依序跑
   `capture-update-templates.py`、`capture-skills.py`、`parity-probe.py`。建議用 launchd 或
   `/schedule` 每週比對一次 `spectra --version`，有變化才觸發。任何一支 exit 非 0，就開 issue
   並附上保留的 sandbox 路徑。
3. **OpenSpec 行為差異（可以放在 CI）**：在 CI 安裝指定版本的 `@fission-ai/openspec`，把同一組
   fixture 同時餵給 `openspec validate --json` 與 `spectra validate --json`，比對判定結果。
   `docs/openspec-compat.md` 已經把這項列為 deferred；把它做起來之後，OpenSpec 升版造成的分歧
   就會自動浮現，不用靠人去讀 release notes。

### 三、相容性政策

- 預設行為跟隨 oracle 3.0.0。OpenSpec 較嚴格的規則（例如 validate 的 SHALL/MUST），在 #189
  裁決之前維持現狀，不擴大也不縮小。
- 分歧一律列在 `CHANGELOG.md` 與對應的 `docs/reverse-engineering/*.md`；消費端合約欄位不允許
  出現未記錄的分歧（M1 會擋）。
- 目前已知的刻意分歧：trace sidecar（ADR-0029 D3）、validate 預設採 OpenSpec 規則與 OpenSpec
  JSON 格式（D1、D3；oracle 格式可用 `--format oracle` 取得）。切換專案時要讓使用者知道。

## 回退方案

| 情境 | 動作 |
|---|---|
| 單一專案出問題 | 把該專案的 `.spectra/impl` 改成 `oracle`，不用動其他專案 |
| 全面出問題 | 全域設定 `OPENSPECTRA_IMPL=oracle`，或把 `~/.local/bin/spectra` 指回 Spectra.app |
| `update` 改壞 skill | `git revert` 該專案的 update commit；因為 runbook 要求 update 獨立成一個 commit，所以可以乾淨地還原 |
| trace sidecar 已經寫入 | sidecar 是加法：`spec.md` 只多一行 pointer，舊的 inline footer 被吸收進 sidecar。回到 oracle 後，oracle 會繼續寫 inline footer，兩種格式並存但不會遺失資料（推論，要在 Phase 2 試點時實測確認） |

## 裁決紀錄（2026-09-28，howie）

| 項目 | 問題 | 裁決 | 落在本文哪裡 |
|---|---|---|---|
| D1 | validate 嚴格度跟 oracle 還是 OpenSpec（#189） | **先與 OpenSpec 一致**。理由：OpenSpec 有公開原始碼，每條規則都能指到程式碼位置，比黑箱 oracle 容易理解與驗證 | 「validate 嚴格度」三方比對、M4 |
| D2 | B1（模板呼叫未移植的 CLI）怎麼處理 | **(a) 先補齊 CLI 介面**；不做降級模板，補齊前各專案不跑 `update` | M2 |
| D3 | `validate --json` 格式 | **提供 oracle 相容格式並設為預設，直到 OpenSpec 格式完全跟上為止**；之後再把 OpenSpec 格式補齊到逐欄位一致 | M3 |
| D4 | `yibi-mvp-fix-*` | **清掉**；已於 2026-09-28 以 `git worktree remove` 移除（檢查結果見下），分支保留待 `/clean-wt` | Phase 4 |
| D5 | 沒有消費端的 3.0.0 指令要不要移植 | **以後再做**：排在執行佇列最後 | M5 |
| D6 | 實作切換放在哪裡 | **做成 OpenSpectra 內建** | Phase 1 |
| D7 | touched／baseline 模型（W4、W6） | **(a) 以 v3 per-task baseline 取代 #98 per-change baseline** | W4、W6 |

**D8（待裁決，2026-09-28 提出）：`scope` 的 git 存取方式。** oracle 內含 libgit2，`scope`
輸出的 patch 是 libgit2 的格式。RE 實測（`scope` 規格）與 `git diff` 相比有 7 種位元組差異：
空的新檔仍有 `---`／`+++` 行、含空白的路徑不加尾端 tab、intent-to-add 檔拆成 staged 與
unstaged 兩段、worktree 內的 rename 會被偵測、50% 相似就算 rename、abbrev 固定 7 碼、巢狀
repo 的 patch 為空字串。`snapshot_id` 則會對 `.git/index` 原始位元組取 hash。選項：
(a) 改用 `git2` crate（vendored libgit2）：與 oracle 逐位元組相同，但新增原生相依，會影響
Linux musl 靜態編譯與 crates.io 發佈；(b) 維持 git CLI：在這 7 種邊角情況記為刻意分歧，
一般情況（文字檔的修改、新增、刪除）仍然相同；(c) 只有 `scope` 用 `git2`、其餘維持 CLI。

**D7（2026-09-28 howie 裁決：(a) 以 v3 取代 #98）：W4 的 baseline 模型。** W6 的
`scope`（實作前基準與 touched 歸屬）同樣依賴這個模型，所以依序做 W4 → W6。原始選項說明： oracle 3.0.0 的 `task start`
寫 `.spectra/task-baselines/<change>/<id>.json`；`task done` 只在有 task baseline 或
`--file` 時記錄 touched files，否則發警告並跳過，而且 `new change` 不再寫任何
`.spectra/` 狀態（W3 實測）。OpenSpectra 的 #98 則由 `new change` 寫 per-change
baseline，每次 `task done` 都記錄。選項：(a) 以 v3 取代 #98（與 oracle 完全一致，
`archive` 的 `code` trace 改由 task baseline／`--file` 提供，沒跑 `task start`
的專案會拿到空的 `code`）；(b) 兩者並存，沒有 task baseline 時退回 #98（`code`
不會變空，但 `task done` 的警告與 touched 結果和 oracle 不同）；(c) 維持 #98 為
刻意分歧，`task start` 只做介面相容。建議 (a)，理由是驗收條件 A3 要求輸出一致。

**D4 執行前檢查（實測）**：四個目錄是 yibi-mvp 的 linked worktree，都沒有未 commit 的改動；
fix-1826／1827／1828 本機的每個 commit 在各自遠端分支上都有等價 patch（`git cherry` 0 unmatched）；
fix-1833 沒有 upstream，但它獨有的 3 個 commit 都在已 merge 的 PR #2010 的 commit 清單裡。
所以**已追蹤的內容不會遺失**。會跟著刪掉的是被 gitignore 的本機檔案：

- fix-1827 的 `mobile/.env` 與 yibi-mvp 主目錄的**不同**（其他 `.env` 都相同）。
- 每個目錄底下有約 10 個 `.claude/worktrees/*` 子目錄，它們**沒有**註冊在 yibi-mvp 的
  `git worktree list` 裡，推測是建立 fix 目錄時連同主目錄的 `.claude/worktrees/` 整份複製過來的舊副本。
- 各自的 `.spectra/`（165–168 個檔）、`.pr-review/`、`.runtime/`。

刪除方式：在 yibi-mvp 主目錄執行 `git worktree remove <path>`（不加 `--force`，有未 commit 的改動時會拒絕），
分支先保留，之後交給 `/clean-wt` 處理。

**D5 說明**：「沒有消費端」指的是在以下所有會呼叫 `spectra` 的地方，都找不到這些指令或 flag：
3.0.0 skill 模板（6 家 AI 工具共 72 個檔）、yibi-mvp 現有的 skill、yibi-stack plugin
（`sdd`、`dev-cycle`）、yibi-mvp 的 CI 與 scripts（搜尋有正向對照：同一組路徑搜得到 `spectra scope` 26 次）。
也就是說，目前沒有任何自動化流程會呼叫它們，只有人在終端機手動打才會用到。缺少這些功能的影響：

| 缺少的功能 | oracle 的用途 | OpenSpectra 缺少時的影響 |
|---|---|---|
| `decisions` | 列出各 change `design.md` 的架構決策 | 人要自己翻 `design.md`；skill 不受影響 |
| `show --deltas-only`／`--requirements`／`--item-type` | 只顯示 delta、只顯示 requirement、指定是 change 還是 spec | 手動用時會得到 `unexpected argument`；OpenSpectra 有 `show --diff` 可部分替代 |
| `demo` | 產生一個示範 change | 無實際影響 |
| `feedback` | 回報意見給 Spectra 作者 | 無影響，而且對 OpenSpectra 沒有意義（回報對象不同） |

唯一的風險是**未來**的 oracle 模板開始呼叫它們。M1 的「模板 CLI 解析檢查」會在那時立刻失敗，
所以晚點做不會被靜默漏掉。

## 建議開的新 issue（尚未開立）

以下缺口在本次盤點時沒有找到對應的 issue：

1. `instructions` 缺 `--omit-context`／`--compact`／`--summary`／`--agent`／`--type`（B1，呼叫次數最多）
2. `new change` 缺 `--agent`／`--description`／`--schema`，以及 `no-spec` 內建 schema（B1）
3. `archive` 缺 `--preview`／`--json`（B1）
4. `list`／`list --specs`／`show`／`drift`／`instructions` 的 JSON 欄位與 3.0.0 不一致（B2）
5. `instructions` 的 `locale` 沒有把 `tw` 對應成 `Traditional Chinese (繁體中文)`（B3）
6. `status`／`schemas` 的 artifact 順序仍是 3.0.0 之前的版本（B2）
7. M1：parity probe 與模板 CLI 解析檢查
8. `upstream-watch.yml` 加上版本落差檢查
9. D1：validate 與 OpenSpec 1.13.2 對齊（MODIFIED 目標不存在降為 INFO、0098 的多報），#183 併入
10. D3：`validate --format oracle|openspec`，openspec 格式補齊 `version`／`total`／`root.source` 三處差異
11. D6：內建實作切換（`OPENSPECTRA_IMPL`、`.spectra/impl`、shadow 模式、`spectra impl`）

## 附錄 A：重現本文的實測

以下都在 job 暫存目錄執行，不會碰原始專案。`$OSS` 指 OpenSpectra 的 release build，
`$ORACLE` 指 `/Applications/Spectra.app/Contents/MacOS/spectra`。

```sh
# 1. 建 corpus 副本（以 yibi-mvp 為例）
mkdir -p "$TMP/probe" && rsync -a yibi-mvp/{.spectra.yaml,.claude,CLAUDE.md} "$TMP/probe/"
mkdir -p "$TMP/probe/docs" && rsync -a yibi-mvp/docs/openspec "$TMP/probe/docs/"
git -C "$TMP/probe" init -q && git -C "$TMP/probe" add -A && git -C "$TMP/probe" commit -qm base

# 2. update 的副作用
"$OSS" update "$TMP/probe" && git -C "$TMP/probe" diff --stat

# 3. 唯讀指令比對（兩邊各跑一次，比對 exit code 與輸出）
for args in "list --json" "list --specs --json" "validate --changes --json" \
            "show <change> --json" "status --change <change> --json" \
            "instructions apply --change <change> --json" "drift <change> --json"; do
  "$ORACLE" $args > oracle.out; "$OSS" $args > oss.out   # 再以 JSON 語意比對
done

# 4. 模板裡用到、但 OpenSpectra 缺少的 flag
grep -rhoE 'spectra [a-z-]+[^`]*--(omit-context|compact|summary|agent|type|file|preview|schema)\b' \
  crates/spectra-core/assets/update/ | sort | uniq -c
```

注意：`status --all` 與 `validate --strict` 是 OpenSpectra 才有的 flag，oracle 會回 exit 2，
比對時要排除。
