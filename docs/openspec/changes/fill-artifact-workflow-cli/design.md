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
created_by: howie <howie.yu@gmail.com>   # git user.name <user.email>
```

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

### `new artifact --json`（單行 compact）

```json
{"artifact":"proposal","change":"demo-feature","path":"/abs/path/proposal.md","status":"created","validated":true,"warnings":[]}
```

錯誤字串（stderr，exit 1）：

- `Error: Unknown artifact type 'bogus'. Valid types: proposal, design, tasks, spec`
- `Error: Capability name is required for spec type. Usage: spectra new artifact spec <capability> --change <name>`
- `Error: Artifact already exists: <abs path>. Use --force to overwrite`
- `Error: Proposal must contain a ## Why, ## Problem, or ## Summary section`
  （proposal 驗證；design/tasks/spec 的驗證規則**待 probe**——用 binary
  strings mining `Must contain` 類訊息 + 實測確認）

無 `--stdin` 時以該 type 的空模板建檔（模板 = instructions 的 `template` 欄位）。
spec type 路徑為 `specs/<capability>/spec.md`。

### `instructions <artifact> --json`（pretty、camelCase）

keys：`changeName, artifactId, schemaName, changeDir, outputPath, description,
instruction, locale, template, dependencies, unlocks`。

- `dependencies`：物件陣列 `{id, done, path, description}`（path 為相對）。
- `unlocks`：字串陣列；實測 proposal（全空專案）為 `["design","specs"]`，
  specs（tasks 已 done 時）為 `[]` ——**待 probe**：unlocks 是否只列「尚未
  done」的下游（用 tasks 未建時的 specs 重測）。
- `instruction`/`template`：逐字模板，goldens 已採集
  （`oracle-instructions-{proposal,design,specs,tasks}.json`）。

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
