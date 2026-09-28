# OpenSpectra 導入計畫：從 Spectra.app 遷移到各專案

> 狀態：**proposed**（2026-09-28 起草，待人類裁決「需要裁決的事項」一節後再執行）。
> 本文的「實測」都有附重現指令（見附錄）；標「推論」的是尚未驗證的判斷。

## 背景與目標

各專案目前透過 `~/.local/bin/spectra` 這個 symlink 使用 closed-source 的
`/Applications/Spectra.app/Contents/MacOS/spectra`（實測 `spectra 3.0.0 (Apple Silicon)`）。
本計畫要做到四件事：

1. **換掉**：各專案日常使用的 `spectra` 改由 OpenSpectra 提供，而且過程可隨時回退。
2. **不中斷**：不讓 Claude Code skill、yibi-stack plugin、CI 在切換當下壞掉。
3. **持續監控**：切換後，OpenSpectra 的錯誤與行為分歧要能被主動發現，不能等到使用者撞到才知道。
4. **跟上上游**：持續追蹤 oracle（`kaochenlong/spectra-app`）與 OpenSpec（`Fission-AI/OpenSpec`）的差異，在兩者之間盡量保持相容。

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
| `heyu-ai/yibi-mvp-fix-1826/1827/1828/1833` | yibi-mvp 的整份複本（不是 git worktree） | 13 | 跟隨 yibi-mvp，或直接清掉 |
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

- `validate --changes`：oracle 判 **0/21** 無效（exit 0），OpenSpectra 判 **9/21** 無效（exit 1）。
  這就是 #189「oracle 3.0.0 與 OpenSpec 分歧」待裁決的內容。任何在 archive 前先跑 validate 的
  skill，切換後會卡在這 9 個 change。
- `instructions`：OpenSpectra 有讀 `.spectra.yaml` 的 `locale: tw`（`config.rs:14`），但輸出的
  `locale` 仍是 `English`，oracle 則是 `Traditional Chinese (繁體中文)`。這會讓 agent 用錯語言寫 artifact。
- `analyze`：dimension 數量不同（oracle 5 個，OpenSpectra 4 個），各 dimension 的 finding 數量也不同（#169）。

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
3. **切換必須可以一鍵回退。** 用 shim 決定 `spectra` 指向哪個實作，不直接改 symlink（見 Phase 1）。
4. **先 shadow 再切換。** 先讓 OpenSpectra 在背景跑唯讀指令並記錄差異，差異收斂之後才讓它成為主要實作。
5. **由低風險到高風險，逐個專案切換**，每個專案切換後至少觀察一週再換下一批。

## 分階段計畫

```
Phase 0（openspectra repo：補阻擋項 + parity 基礎建設）
   └─> Phase 1（本機 shim + shadow 模式）
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
每一項都要先抓 oracle golden（依本 repo 慣例）。在全部完成之前，也可以考慮讓 `update` 暫時改寫
模板、只呼叫已支援的 flag。這是架構取捨，**需要人裁決**（見下方「需要裁決的事項」D2）。

**M3. 解決 B2**：`list`／`list --specs`／`show`／`drift`／`instructions` 的 JSON 欄位補齊到 3.0.0。
`validate --json` 的形狀比較特殊：OpenSpectra 刻意採用 OpenSpec v2 格式（README 有寫），
要決定是否提供 oracle 相容格式（例如 `--format oracle`，或反過來把 v2 放在 flag 後面），**需要人裁決**（D3）。

**M4. 解決 B3**：`locale` 的對應（`tw` → `Traditional Chinese (繁體中文)`）、`analyze` 的 dimension
（#169）；`validate` 嚴格度依 #189 的裁決處理。

**M5. 其他 3.0.0 缺口**：`decisions`（#166）、`show` 的三個 flag、`demo`（#62）。這些目前沒有
消費端，優先度最低。依 `CLAUDE.md` 的規定，移植前要先確認有消費者，沒有就回報給人決定要不要做。

**Phase 0 出口條件**：parity probe 在 yibi-mvp、nextrek-cli、yibi-stack 三個 corpus 上，
消費端合約欄位 0 分歧；模板 CLI 解析檢查在 CI 上全綠；發一個 release。

### Phase 1 — 本機 shim 與 shadow 模式

1. 從 release tarball 安裝 OpenSpectra 到 `~/.local/opt/openspectra/<version>/spectra`，
   **不要**直接覆蓋 `~/.local/bin/spectra`。
2. 把 `~/.local/bin/spectra` 換成一支 shim script，由環境變數或設定檔決定要用哪個實作：
   - `SPECTRA_IMPL=oracle`（Phase 1 的預設）：執行 Spectra.app。
   - `SPECTRA_IMPL=shadow`：以 oracle 的結果為準回傳；如果是唯讀指令（`list`、`show`、`status`、
     `validate`、`analyze`、`drift`、`instructions`、`schemas`、`templates`），就在背景用 OpenSpectra
     再跑一次，輸出不同時寫一筆 JSONL 到 `~/.local/state/openspectra/shadow.jsonl`
     （內容：時間、cwd、argv、兩邊的 exit code、diff 摘要）。
     **會寫檔的指令（`archive`、`task done`、`new`、`park`、`update`……）絕對不能 shadow**，
     因為兩個實作會對同一份檔案各寫一次。
   - `SPECTRA_IMPL=oss`：執行 OpenSpectra，同時記錄非 0 exit、stderr、panic。
   - 可以用專案層級的 `.spectra-impl` 檔覆寫（讓 Phase 2 逐專案切換）；回退就是改回 `oracle`。
3. 本機先全域開 `shadow` 一週，累積真實使用下的差異樣本。

**Phase 1 出口條件**：shadow log 裡沒有未分類的消費端合約分歧。每一筆分歧都已經分類為
「開了 issue」「刻意分歧」或「不影響消費端」。

### Phase 2 — 試點（低風險）

依序：`side-project/MiniShell` → `ainization-skill` → `heyu-ai/yibi-stackchan` →
`heyu-ai/storysonic-lab` → `openab-projects/openab-console`。每個專案照下面的 runbook 做。

#### 每個專案的切換 runbook

1. **前置**：確認沒有其他 session 正在該專案跑 spectra 流程；從 `origin/main` 開分支。
2. **備份受管區塊內的手寫內容**：`CLAUDE.md`／`AGENTS.md` 在 `<!-- SPECTRA:START -->` 與
   `<!-- SPECTRA:END -->` 之間，如果有人手動加的內容，先搬到區塊外。`update` 會整段覆寫這個區塊。
3. **寫入 `.spectra-impl` 內容為 `oss`**（這個檔要加進 `.gitignore`，它是本機設定）。
4. **只讀驗證**：`spectra list --json`、`spectra validate --changes`、`spectra status --change <x> --json`，
   並對照 oracle 的輸出（`SPECTRA_IMPL=oracle` 再跑一次）。
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
3. **`yibi-mvp-fix-18xx` 複本**：它們是整份 clone，不是 worktree。建議確認沒有未推送的工作後
   直接清掉，不要逐一遷移（**需要人確認**）。
4. **yibi-stack plugin**：plugin 呼叫 `spectra archive <name> --yes`，兩個實作都支援。
   但 plugin 文件應註明「支援 OpenSpectra ≥ vX」，而且 plugin 自己的 CI 應該要能用 OpenSpectra
   跑它文件裡的範例指令。

### Phase 5 — 把 Spectra.app 移出 PATH

條件：所有專案都已經切到 `oss` 至少兩週，shadow／oss log 裡沒有未處理的合約分歧。

1. shim 的預設值改成 `oss`，接著移除 shim 裡 `oracle` 的預設路徑（仍保留用 `SPECTRA_IMPL=oracle`
   手動切換的能力）。
2. Spectra.app **保留在 `/Applications`**，但關掉它的自動更新（推論：它有自己的更新機制；
   若會自動更新，就會讓 golden 在不知情的情況下換版本）。它的角色改成純粹的 oracle，
   只有校準腳本會用到。
3. 在 `~/.claude/CLAUDE.md` 或各專案的文件中，把「spectra」的說明改成指向 OpenSpectra。

## 持續監控

### 一、OpenSpectra 自己的錯誤

| 來源 | 做法 | 頻率 |
|---|---|---|
| 本機 shim | `oss` 模式記錄非 0 exit、stderr、panic 到 `~/.local/state/openspectra/errors.jsonl`；`shadow` 模式記錄分歧到 `shadow.jsonl` | 即時 |
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
- 目前已知的刻意分歧有兩項：trace sidecar（ADR-0029 D3）與 validate v2 JSON。切換專案時要讓使用者知道。

## 回退方案

| 情境 | 動作 |
|---|---|
| 單一專案出問題 | 把該專案的 `.spectra-impl` 改回 `oracle`，不用動其他專案 |
| 全面出問題 | `SPECTRA_IMPL=oracle`，或把 shim 的預設值改回 oracle |
| `update` 改壞 skill | `git revert` 該專案的 update commit；因為 runbook 要求 update 獨立成一個 commit，所以可以乾淨地還原 |
| trace sidecar 已經寫入 | sidecar 是加法：`spec.md` 只多一行 pointer，舊的 inline footer 被吸收進 sidecar。回到 oracle 後，oracle 會繼續寫 inline footer，兩種格式並存但不會遺失資料（推論，要在 Phase 2 試點時實測確認） |

## 需要裁決的事項

以下每一項都是人的決策，agent 不應自行選定方向：

- **D1. #189：validate 嚴格度。** oracle 判 0/21 無效，OpenSpectra 判 9/21 無效。
  預設要跟 oracle，還是跟 OpenSpec？這會直接決定 yibi-mvp 切換時有多少個 change 需要修。
- **D2. B1 的處理方式。** 選項 (a)：先補齊 CLI 介面，才允許各專案 `update`（慢，但忠實）；
  選項 (b)：`update` 暫時產生「降級版」模板，只呼叫已支援的介面（快，但會跟 oracle 模板分歧，
  而且之後要再改回來）；選項 (c)：切換後各專案都不跑 `update`，繼續用舊 skill（最快，但舊 skill
  本身已經有 `spectra sync` 這種失效呼叫，也拿不到 3.0.0 的改進）。
- **D3. `validate --json` 的格式。** 維持 OpenSpec v2 格式，還是提供 oracle 相容格式？
  哪一種應該是預設？
- **D4. `yibi-mvp-fix-18xx` 複本**：清掉還是保留？
- **D5. 移植沒有消費端的 3.0.0 指令**（`decisions`、`demo`、`feedback`、`show` 的三個 flag）：
  要不要做？依 `CLAUDE.md` 規定，這類問題要先問。
- **D6. shim 放在哪裡**：放在 `~/.local/bin`（只影響這台機器），還是做成 OpenSpectra 內建的
  `spectra --impl` 機制（其他人也能用，但 OpenSpectra 就要知道 oracle 的存在）？

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
