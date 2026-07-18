# Design：fill-artifact-workflow-cli

> 所有 oracle 事實均為 2026-07-18 對 Spectra.app 2.3.1（`~/.local/bin/spectra`）
> 在乾淨 probe 專案的實測結果；標注「待 probe」者為實作 WP 時必須先補測的項目。

## 架構總覽

```
spectra-core/
  schema.rs        # WP1：spec-driven schema 定義 + DAG 狀態推導（純函式）
  artifact.rs      # WP2：new artifact scaffold + per-type 內容驗證
  instructions.rs  # WP3：instructions 資料組裝（引用 schema.rs 的模板常數）
  analyze.rs       # WP4：4 維度 findings engine
spectra-cli/main.rs  # 各 WP 各自加一個子指令 arm
```

依賴方向：WP2/WP3/WP4 都依賴 WP1 的 schema 模組；WP2–WP4 彼此獨立。
apply 模式（WP3）另在 `instructions.rs` 實作 oracle 較寬鬆的 checkbox 解析，
並複用 `git.rs` 執行 preflight 的每檔最後 commit 日期查詢；既有
`tasks.rs` 的嚴格 parser 不變。

## 關鍵決策

### D1：schema 內建為 Rust 常數，不做外部 schema 檔

oracle 有 `schemas` 指令與「package」概念，但只內建 `spec-driven` 一個。
本次把 schema 定義（artifact id、outputPath、deps、instruction/template 文字）
寫成 `schema.rs` 內的常數表。模板文字自 oracle `instructions <a> --json` 逐字
擷取（goldens 見 tasks.md WP1 的採集步驟），以 raw string 常數嵌入。
被否決方案：讀外部 YAML schema 檔——oracle 的 package 格式未 RE、sdd plugin
不需要，過度設計。

### D2：狀態推導是純檔案存在性，無持久狀態

實測：`done` = outputPath（相對 change dir 的 glob，如 `specs/**/*.md`）至少
命中一檔；`ready` = 非 done 且所有 deps 皆 done；`blocked` = 非 done 且任一
dep 未 done（`missingDeps` 列出未 done 的 deps）。刪除 `specs/` 後 `tasks`
仍為 `done`（檔案還在）——狀態彼此獨立、無 cascade，不需要任何 state file。
`isComplete` = 全部 artifacts done。`applyRequires` 固定 `["tasks"]`。

### D3：`new change` 對齊 oracle——不 scaffold artifact 檔（BREAKING）

oracle `new change` 只建 change dir + `.openspec.yaml`：

```yaml
schema: spec-driven
created: 2026-07-18
created_by: howie <howie.yu@gmail.com>   # git user.name <user.email>
```

`created_by` 永遠不省略，fallback matrix 實測如下：

- `user.name` 與 `user.email` 皆有值：`"name <email>"`
- 僅 `user.name` 有值：`"name"`
- 僅 `user.email` 有值：`"<email>"`
- `created_by` 取自 git config（優先使用 repo-local 設定，未設定時回退至
  使用者層級的 global 設定；即使不在 git repo 內亦同）；只有在所有層級都無法
  取得 `user.name` 與 `user.email` 時，才寫入 `"unknown"`

oracle 不會寫入 `created_with` key。

openspectra 現行 scaffold proposal.md/design.md/tasks.md 會讓 status 一開始
全 `done`、`new artifact` 永遠撞 already-exists。對齊 oracle：移除 scaffold，
`.openspec.yaml` 增寫 `schema`/`created`/`created_by`（保留 openspectra 既有的
started_sha 機制——它存在 `.spectra/changes/<name>.started`，與本檔無關，
drift 依賴它，不動）。既有整合測試須同步改。

### D4：JSON 命名風格依 oracle，各指令不一致是「特徵」

- `status --json`／`instructions --json`：**camelCase**（`changeName`、
  `missingDeps`、`applyRequires`）
- `analyze --json`：**snake_case**（`change_id`、`finding_count`、
  `artifacts_missing`）
- `new artifact --json`：小寫單詞（`artifact`/`change`/`path`/`status`/
  `validated`/`warnings`），且輸出為**單行 compact JSON**（其餘三者 pretty）。

一律照抄 oracle，不得統一風格（faithful-repro 原則，同 task_done_json 前例）。

### D5：exit code 語義

- `analyze`：恆 exit 0（實測含 findings 時亦然；同 drift 慣例）。
- `new artifact`：驗證失敗／already exists／未知 type → stderr `Error: ...`
  + exit 1。
- `status`/`instructions`：操作性錯誤（change 不存在等）exit 1；正常 exit 0。

## Oracle 合約（實測 golden shapes）

### `status --json`

```json
{
  "changeName": "demo-feature",
  "schemaName": "spec-driven",
  "isComplete": false,
  "applyRequires": ["tasks"],
  "artifacts": [
    { "id": "proposal", "outputPath": "proposal.md", "status": "ready" },
    { "id": "design",  "outputPath": "design.md",  "status": "blocked",
      "missingDeps": ["proposal"] },
    { "id": "specs",   "outputPath": "specs/**/*.md", "status": "blocked",
      "missingDeps": ["proposal"] },
    { "id": "tasks",   "outputPath": "tasks.md", "status": "blocked",
      "missingDeps": ["specs"] }
  ]
}
```

`missingDeps` 僅在 `blocked` 時出現；`done` 時無該欄位。human 輸出：

```
Change: demo-feature
Schema: spec-driven

  ✓ proposal (proposal.md)     # done
  ○ design (design.md)         # ready
  ✗ tasks (tasks.md)           # blocked
    blocked by: specs
```

status 的依賴探測亦確認 `tasks.deps = ["specs"]`，`design` 不是 tasks 的
dependency；空 change 時 tasks 的 `missingDeps` 只有 `["specs"]`。

操作性錯誤沿用 oracle 的逐字訊息（stderr 由 CLI 再加上 `Error: `）：

- change 不存在：`Change 'nope' not found.`
- schema 不存在：`Schema not found: Schema 'bogus' not found in project, user, or built-in locations`

human 輸出的結尾也已補 probe：尚未完成時 artifact 清單後保留一個空白行，
因此最後兩個 bytes 為 `\n\n`；全部完成時則在該空白行後追加最後一行
`  ✓ All artifacts complete`，並以單一換行結束。

### `new artifact --json`（單行 compact）

```json
{"artifact":"proposal","change":"demo-feature","path":"/abs/path/proposal.md","status":"created","validated":true,"warnings":[]}
```

錯誤字串（stderr，exit 1）：

- `Error: Unknown artifact type 'bogus'. Valid types: proposal, design, tasks, spec`
- `Error: Capability name is required for spec type. Usage: spectra new artifact spec <capability> --change <name>`
- `Error: Artifact already exists: <abs path>. Use --force to overwrite`
- `Error: Proposal must contain a ## Why, ## Problem, or ## Summary section`
  （content 小寫化後含 `## why`/`## problem`/`## summary` 任一 substring 即通過，
  行中出現亦可；已實測）
- `Error: Design must contain a ## Context section`（同上，substring `## context`）
- `Error: Tasks must contain at least one checkbox (- [ ])`（content 含 `- [ ]`、
  `* [ ]`、`+ [ ]` 任一 literal 即通過；已實測）
- `Error: Delta spec parse error: Invalid format: Delta spec must contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)`
  （需有一行 trim 後**恰等於** `## ADDED/MODIFIED/REMOVED/RENAMED Requirements`；
  大小寫敏感、可縮排、不可有後綴；requirement 缺 scenario、REMOVED 缺
  Reason/Migration 都不擋，驗證僅到 operation heading 層級）
- `Error: Invalid capability name '<cap>'. Must be kebab-case (e.g., user-auth, data-export)`
  （合法字元 `[a-z0-9-]`、不得以 `-` 開頭或結尾；`a--b`、`cap2-v3` 合法）
- `Error: No content received from stdin`（`--stdin` 內容為空或僅空白）
- `Error: Change '<name>' not found`（**無句尾句點**；與 `status` 的
  `Change 'x' not found.` 不一致，oracle 即如此）

實測補充（2026-07-18 probe）：

- 檢查順序：change 自動解析（無 `--change` 時的 no-active/multiple 錯誤）→
  unknown type → change 存在性 → capability 必填/kebab → already exists
  （`--force` 跳過）→ 空 stdin → 內容驗證 → 寫檔。已存在錯誤先於空 stdin
  與內容驗證；驗證失敗不寫任何檔案或目錄。
- `validated`：`--stdin` 內容通過驗證為 `true`；模板建檔（無 `--stdin`）
  一律 `false`（模板不跑驗證，即使模板內容本身含必要 section）。
- `warnings`：所有 probe 情境（`--force` 覆蓋、deps 未 done、requirement 缺
  scenario 等）皆為 `[]`，未觀察到非空案例。
- `--force` 不跳過內容驗證：無效內容加 `--force` 仍 exit 1 且不覆蓋原檔。
- stdin 內容逐位元寫入，不補結尾換行；compact JSON 輸出後有一個換行。
- human 輸出：`✓ Created <type>: <abs path>`；validated 時追加第二行
  `  Content validated ✓`（兩格縮排）。
- oracle 的 multiple-changes 錯誤為 `Multiple changes found. Use --change to
  specify one: <names>`（依 mtime 新到舊排序）；openspectra `change::resolve`
  現行為 `Use a change name to specify one:`（字母排序）。WP2 沿用 resolve
  不改，差異留待主 session 裁決。

無 `--stdin` 時以該 type 的空模板建檔（模板 = instructions 的 `template` 欄位），此時 `validated` 為 `false`。
spec type 路徑為 `specs/<capability>/spec.md`。

### `instructions <artifact> --json`（pretty、camelCase）

JSON 為 2-space pretty 格式並保留結尾換行，top-level keys 固定依序為
`changeName, artifactId, schemaName, changeDir, outputPath, description,
instruction, locale, template, dependencies, unlocks`。`changeDir` 為 change
目錄的絕對路徑；`outputPath` 保留 schema 內的相對路徑（例如
`specs/**/*.md`）；`locale` 固定為 `English`；`instruction`/`template` 逐字
使用 `schema.rs` 內已由 goldens 釘住的常數。

- `dependencies` 依 artifact 的 deps/schema 順序輸出，每個物件 key 依序為
  `{id, done, path, description}`。`done` 採 WP1 的檔案存在規則：specs 只要
  `specs/` 下遞迴存在任一 `.md` 即完成，其餘 artifact 以檔案是否存在判定；
  `path` 與 `description` 取自 dependency 本身的 schema 定義。
- `unlocks` 只列 direct dependents，且必須同時符合：目前 artifact 尚未
  done、dependent 亦尚未 done。順序固定為 schema 順序。實測矩陣：空 change
  的 proposal → `["design","specs"]`、空 change 的 specs → `["tasks"]`；
  proposal 已完成後再查 proposal → `[]`；proposal/design/tasks 已完成但 specs
  未完成時，specs → `[]`；proposal 缺少但 specs/design 已完成時，proposal →
  `[]`；只有 specs 完成時，proposal → `["design"]`。

human 輸出不顯示 preflight；只有 artifact 有 deps 時才出現 `Dependencies:`，
完成用 `✓`，未完成用 `○`。`unlocks` 非空時另顯示 `Unlocks:`，每項格式為
`  - <id>`；此區段位於 `Dependencies:` 之後、`Template:` 之前，空陣列時整段
省略。兩區段彼此獨立，可只出現其中一個：

```text
Artifact: specs
Output: specs/**/*.md
Description: Detailed specifications for the change

Instruction:
<instruction 原文>

Dependencies:
  ○ proposal (proposal.md)

Unlocks:
  - tasks

Template:
<template 原文>
```

操作性錯誤逐字如下（CLI 另加 `Error: ` 並 exit 1）：

- 未知 artifact：`Artifact '<id>' not found in schema`（無句尾句點）
- change 不存在：`Change '<name>' not found.`（有句尾句點）
- 未知 schema：`Schema not found: Schema '<s>' not found in project, user, or built-in locations`
- `--skill <name>`：`Unknown skill: <name>`。oracle 對其內建有效名稱會輸出
  proprietary embedded skill bodies；openspectra 刻意不移植，所有名稱皆視為
  unknown。這是已記錄的產品差異；`--skill` 的錯誤優先於其他參數檢查。

### `instructions`（無參數）／`instructions apply` —— apply 模式

無 artifact 參數時，不是一律進 apply：若四個 artifacts 皆 done 才選 apply；
否則依 schema 順序 `proposal, design, specs, tasks` 選第一個未 done artifact，
並輸出上一節的 artifact mode。

apply JSON 的 top-level keys 固定依序為 `changeName, changeDir, schemaName,
contextFiles, progress, tasks, state, missingArtifacts, locale, instruction,
preflight`；其中 `missingArtifacts` 為空時省略，`preflight` 只在 `state ==
"ready"` 時出現。`instruction` 固定為：

```text
Read context files, work through pending tasks, mark complete as you go.
Pause if you hit blockers or need clarification.
```

`contextFiles` 只含已 done 的 artifact，值為絕對路徑；specs 使用絕對 glob
`<changeDir>/specs/**/*.md`。oracle 以 hash map 輸出，key order 不穩定；
openspectra 刻意固定為 `proposal, design, specs, tasks` 的 schema 順序，以提供
可重現輸出，這是 determinism 選擇而非語義差異。

apply 使用獨立的寬鬆 parser（不得改動 `tasks.rs`）：

```regex
^\s*[-*+]\s*\[(.)\]\s*(.+)$
```

- `-`、`*`、`+` 均可作 bullet；checkbox 內任一單一字元（包括 `z`、`?`、
  `P`）都算 task，只有 `x`/`X` 是 done。
- `id` 是依檔案順序產生的 1-based 字串（`"1"`, `"2"`, ...），
  `description` 為 checkbox 後內容 trim 後的原文。
- checkbox 後緊接（中間可有零個以上空白）的 uppercase literal `[P]` 會被
  從 description 移除並設 `parallel: true`；`[p]` 不算，description 中較後方
  才出現的 `[P]` 也不算。例如 `- [ ][P] x` 為 parallel，而
  `- [ ] 1.1 [P] x` 不是。
- `progress` 為 `{total, complete, remaining}`。`total == 0` 時 state 為
  `blocked`；有 tasks 且 `remaining == 0` 時為 `all_done`；其餘為 `ready`。
- `missingArtifacts` 列 `applyRequires` 中尚未 done 的 artifact，目前即
  `["tasks"]`，且只在非空時輸出。因此 tasks.md 缺少時為 `blocked` 並帶
  `missingArtifacts: ["tasks"]`；tasks.md 存在但沒有 checkbox 時同為
  `blocked`，卻不輸出 `missingArtifacts`。

`preflight` 只在 `ready` 出現，key 順序為 `status, missingFiles,
driftedFiles, staleness`：

- `staleness.daysOld` = 本地日曆日 `Local::now().date_naive()` 減去
  `.openspec.yaml` 的 `created: YYYY-MM-DD`；`daysOld > 7` 才設
  `isStale: true`（7 天為 false、8 天為 true）。未來日期保留負數、不 clamp；
  缺少或無法解析 created 時，整個 `staleness` key 省略，且不做 drift 比對。
- `missingFiles` 僅來自 proposal 的 refs；`root.join(path)` 不存在即輸出
  `{path, referencedIn: "proposal"}`，不依賴 git。
- `driftedFiles` 合併 proposal、design、tasks 的 refs；檔案存在、位於 git
  repo、且 `git log -1 --format=%cs -- <path>` 的日期嚴格晚於 change created
  才輸出 `{path, lastCommit, changeCreated}`。日期相等不算 drift；非 git repo、
  無 commits 或無合法 created 時略過。
- refs 依 proposal → design → tasks 的 first-seen 順序去重。`status` 優先序：
  `missingFiles` 非空為 `critical`；否則 drift 非空或 stale 為 `warnings`；
  其餘為 `clean`。

proposal refs 先將全文 lowercased，再從最早出現的第一個 marker 開始掃描；
marker 為 `affected code:`、
`主要檔案`、`影響檔案`、`變更檔案`、`受影響檔案`。marker 同一行的剩餘內容
先移除 backticks，再用 loose path pattern 搜尋所有含 slash 的允許副檔名 token：

```regex
([A-Za-z0-9_\-./]+/[A-Za-z0-9_\-./]+\.(?:rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js))
```

後續行掃到第一個 trim 後以 `#` 開頭的 heading 為止；空白行不中止。含
backticks 的行以 oracle 的 `BACKTICK_PATH_RE` search-all：

```regex
`([^`]*?/[^`]*?\.(?:rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js))`
```

不含 backticks 的行先移除 bullet，再只移除結尾 ASCII parenthetical
annotation `\s*\([^)]*\)\s*$`，trim 後必須完整符合 prefix whitelist 的
`BARE_PATH_RE`：

```regex
\b((?:specs|src|src-tauri|crates|lib|tests|app|public)/[\w\-/]+\.(?:rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js))\b
```

design.md 與 tasks.md 不做 section 限制，全文只跑 `BACKTICK_PATH_RE`，且只會
成為 drift candidates，不會進 `missingFiles`。允許副檔名精確為
`rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js`；`py, txt, go, yml, sh,
sql, vue, mjs` 皆不接受。

apply human 輸出不顯示 preflight；有 tasks 時列 `Tasks:`，done 用 `✓`，pending
用 `○`，parallel 沒有特殊 glyph；blocked 且 `missingArtifacts` 非空則改列
`Missing artifacts:`。tasks.md 存在但零 checkbox 的 human edge 尚未由 oracle
probe，openspectra 選擇兩個 optional section 都不顯示。其餘格式如下：

```text
Change: demo
Schema: spec-driven
State: ready
Progress: 1/3 complete

Tasks:
  ✓ 1.1 done one
  ○ 1.2 pending two

Instruction:
Read context files, work through pending tasks, mark complete as you go.
Pause if you hit blockers or need clarification.
```

### `analyze [CHANGE] --json`（pretty、snake_case、恆 exit 0）

```json
{
  "change_id": "demo-feature",
  "dimensions": [
    {"dimension": "Coverage", "status": "1 issue(s) found", "finding_count": 1},
    {"dimension": "Consistency", "status": "Clean", "finding_count": 0}
  ],
  "findings": [
    {
      "id": "COV-1",
      "dimension": "Coverage",
      "severity": "Warning",
      "location": "specs/demo-cap/spec.md",
      "summary": "Requirement 'Demo works' has no matching task",
      "recommendation": "Add a task in tasks.md that references 'Demo works'",
      "summary_msg": {"key": "covMissingTask.summary", "params": {"req": "Demo works"}},
      "recommendation_msg": {"key": "covMissingTask.recommendation", "params": {"req": "Demo works"}}
    }
  ],
  "artifacts_analyzed": ["proposal", "specs", "design", "tasks"],
  "artifacts_missing": []
}
```

- dimension `status` 字串：`Clean` / `N issue(s) found` /
  `Skipped (insufficient artifacts)`（artifacts 不足時 4 維度全 skip、
  `findings` 空）。
- finding `id` = 維度縮寫 + 序號（`COV-1`、`AMB-1`…）。
- **finding 全集（binary strings mining，已封閉）**：
  - Coverage：`covMissingSpec`、`covMissingTask`、`covDeltaValidation`
  - Consistency：`conDesignNotInTasks`
  - Ambiguity：`ambNoScenario`、`ambAbstractScenario`、`ambWeakLanguage`
  - Gaps：`gapNoProposal`、`gapNoMainSpec`、`gapModifiedNotFound`
- **finding 實測契約**：`params` 欄依序為
  `summary_msg.params / recommendation_msg.params`；空集合仍輸出 `{}`。

  | finding key | severity | 觸發條件 | params keys | location |
  |---|---|---|---|---|
  | `covMissingSpec` | Critical | Coverage 有執行且 proposal 的 Capabilities token 缺少 change delta `specs/<cap>/spec.md` | `cap / cap` | `proposal.md → Capabilities` |
  | `covMissingTask` | Warning | tasks 全文不含 requirement 名稱（case-insensitive substring） | `req / req` | delta spec 相對路徑 |
  | `covDeltaValidation` | Critical | 同 section 重複 requirement，或同名 requirement 橫跨兩個 operation sections | `error / {}` | delta spec 相對路徑 |
  | `conDesignNotInTasks` | Warning | design 的 level-3 topic（lowercase）未出現在 tasks 全文 | `keyword / {}` | `design.md` |
  | `ambNoScenario` | Warning | requirement 在下一個 requirement／operation heading 前沒有 scenario | `req / req` | delta spec 相對路徑 |
  | `ambAbstractScenario` | Suggestion | scenario 在下一個 scenario／requirement／operation heading 前沒有 example | `scenario / scenario` | delta spec 相對路徑 |
  | `ambWeakLanguage` | Suggestion | spec 某行命中弱語詞規則 | `pattern / pattern` | `<delta spec>:<line>` |
  | `gapNoProposal` | Critical | specs 存在但 `proposal.md` 不存在 | `{}` / `{}` | `change directory` |
  | `gapNoMainSpec` | Warning | delta 有 MODIFIED requirement，但 capability main spec 不存在 | `spec / spec` | delta spec 相對路徑 |
  | `gapModifiedNotFound` | Warning | delta 的 MODIFIED requirement 在既有 main spec 找不到 | `name / name,spec` | delta spec 相對路徑 |

- **Dimension gating**：Coverage 需 `{proposal, specs, tasks}` 至少 2 個；
  Consistency 需 design + tasks；Ambiguity 需 specs；Gaps 需四種 artifact
  至少 1 個。未執行為 `Skipped (insufficient artifacts)`，已執行且無
  finding 為 `Clean`，否則為 `N issue(s) found`。
- **弱語詞規則**：只掃 delta spec files；依 `should`、`may`、`might`、
  `TBD`、`???` 的清單優先序做 case-insensitive plain substring 比對，
  每行最多一筆，回報清單中的 canonical spelling（因此同一行較後面的
  `should` 仍可勝過較前面的 `TBD`）。
- **Capabilities 擷取**：只取 `## Capabilities` section 內 bullet 的第一組
  backtick token；無 backtick 不擷取，`<name>` placeholder 不過濾，也不查
  main `openspec/specs/`，只檢查 change delta 路徑。
- `gapModifiedNotFound` 使用 trimmed requirement name 的 exact equality；
  substring（如 `Login flow` 對 `Login flow extended`）不算命中。
- oracle side-by-side 實測顯示，`gapModifiedNotFound` 的
  `recommendation_msg.params` 兩個 keys（`name`、`spec`）因 oracle 本身的
  hash-map iteration nondeterminism，跨次執行的序列化順序不穩定；
  openspectra 固定為 `name` 後 `spec`。這與 spec-file 排序同屬 determinism
  choice，並非 divergence bug。
- `covDeltaValidation` 與 `conDesignNotInTasks` 的
  `recommendation_msg.params` 是實測的不對稱空 `{}`。
- **Human output**：固定以 `Change: <name>` 開頭，接 4 行
  `  <✓|●> <dimension:15><status> (<N> findings)`；再列 `Analyzed`／
  `Missing`，有 findings 時輸出 `Findings (N):` 與每筆三行
  `[CRITICAL|WARNING|SUGGEST]`、`at:`、`→ recommendation`，否則輸出
  `✓ No issues found`。全程 plain text，`--no-color` 不改變 bytes。
- oracle 的 spec-file 跨檔順序來自 readdir，因而非 deterministic；
  openspectra 依 change-relative path 排序，這是與 WP3 `contextFiles`
  相同的 determinism choice，且每檔內維持 Coverage／Ambiguity 的實測分組。

## 驗證策略

- 每個 WP：單元測試（純函式）＋ `crates/spectra-cli/tests/` 整合測試
  （對照本檔 golden shapes 的逐 key 斷言，含 compact vs pretty、
  camelCase vs snake_case）。
- macOS 本機另跑 oracle side-by-side 抽查（不進 CI；同 capture-golden.sh 慣例）。
- 既有 `new change` 測試依 D3 改寫；`cargo fmt/clippy/build/test` 全綠才算完。
