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
apply 模式（WP3）另複用既有 `tasks.rs`（checkbox 解析）與 `git.rs`（preflight）。

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
created_by: user <user@example.com>   # git user.name <user.email>（範例用中性值）
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
  ○ specs (specs/**/*.md)      # ready
  ✗ tasks (tasks.md)           # blocked
    blocked by: specs
```

（CLI 一律輸出全部四個 artifact，順序同 schema DAG 宣告；
`status_integration.rs` 斷言四列輸出。）

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

keys：`changeName, artifactId, schemaName, changeDir, outputPath, description,
instruction, locale, template, dependencies, unlocks`。

- `dependencies`：物件陣列 `{id, done, path, description}`（path 為相對）。
- `unlocks`：字串陣列；實測 proposal（全空專案）為 `["design","specs"]`，
  specs（tasks 已 done 時）為 `[]` ——**待 probe**：unlocks 是否只列「尚未
  done」的下游（用 tasks 未建時的 specs 重測）。
- `instruction`/`template`：逐字模板，goldens 已採集，落在
  `docs/reverse-engineering/golden/instructions-{proposal,design,specs,tasks}-2.3.1.json`。
- goldens 採集狀態（provenance）：2026-07-18 對 Spectra.app 2.3.1，probe 專案
  四個 artifact 全數已建——故 `dependencies[].done` 皆為 `true`、`unlocks` 皆為
  `[]`，反映的是「全 done」狀態而非上面「全空專案」的 probe；被釘住的合約僅
  `description`/`instruction`/`template`/`outputPath`/`dependencies[].id`
  （見 schema.rs `embedded_instruction_text_matches_oracle_goldens_byte_for_byte`）。
  `changeDir` 已正規化為 `/tmp/oracle-probe/...`（中性化，非採集原值）。

### `instructions`（無參數）／`instructions apply` —— apply 模式

keys：`changeName, changeDir, schemaName, contextFiles{proposal,design,specs,tasks
→ 絕對路徑/glob}, progress{total,complete,remaining}, tasks[{id,description,done,
parallel}], state, locale, instruction, preflight{status,missingFiles,driftedFiles,
staleness{daysOld,isStale}}`。

- `tasks` 複用 `tasks.rs` 解析；`id` 為字串序號、`description` 含 `1.1` 前綴、
  `parallel` **待 probe**（推測對應 `[P]` 標記）。
- `state`：實測 `ready`；其他值（如全完成後）**待 probe**。
- `preflight`：drift-lite。`staleness.daysOld` 以 `.openspec.yaml` `created`
  計；`missingFiles`/`driftedFiles` 判準**待 probe**（推測 = 缺 artifact 檔
  與 anchors 檢查的輕量版）。
- `--skill <SKILL>` 旗標：oracle help 有（「outputs skill body directly」），
  sdd plugin 未用——收下但回「not supported」錯誤或直接不實作此旗標，
  由 WP3 實作者依 probe 成本決定並記錄於 RE 文件。

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
- 每種 finding 的觸發條件、severity、summary/recommendation 措辭與 params
  keys **待 probe**：對每一種構造正反 fixture 實測（已知 2 種：
  covMissingTask=Warning、ambAbstractScenario=Suggestion；proposal 宣告的
  capability 無對應 spec 檔應為 covMissingSpec，oracle instructions 文字說
  它是 Critical）。
- severity 分級（Critical/Warning/Suggestion）與 human 輸出格式待各 fixture
  一併採集。

## 驗證策略

- 每個 WP：單元測試（純函式）＋ `crates/spectra-cli/tests/` 整合測試
  （對照本檔 golden shapes 的逐 key 斷言，含 compact vs pretty、
  camelCase vs snake_case）。
- macOS 本機另跑 oracle side-by-side 抽查（不進 CI；同 capture-golden.sh 慣例）。
- 既有 `new change` 測試依 D3 改寫；`cargo fmt/clippy/build/test` 全綠才算完。
