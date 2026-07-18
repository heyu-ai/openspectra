# Tasks：fill-artifact-workflow-cli

> WP1 為基礎，WP2–WP4 依賴 WP1 但彼此獨立。每個 WP 完成定義：
> 對應 capability spec 的 scenarios 有測試覆蓋、design.md「待 probe」項目
> 已實測並回填、RE 文件同步、`cargo fmt/clippy/build/test` 全綠。

## WP1 — workflow schema 基礎 + `spectra status`（capability: `workflow-status`）

- [ ] 1.1 採集 goldens：oracle `instructions <a> --json` × 4 artifacts 的
      instruction/template 逐字擷取，嵌入 `schema.rs` 常數
- [ ] 1.2 `spectra-core::schema`：spec-driven DAG 定義 + `done/ready/blocked`
      狀態推導純函式（含 `specs/**/*.md` glob 判定）
- [ ] 1.3 `new change` 對齊 oracle（D3，BREAKING）：移除 artifact scaffold、
      `.openspec.yaml` 增寫 `schema`/`created`/`created_by`；改寫既有測試
- [ ] 1.4 CLI `status [--change] [--schema] [--json]`：human + JSON 輸出
      對齊 design.md 合約（camelCase、missingDeps 條件出現）
- [ ] 1.5 整合測試：DAG 四狀態轉移（空 → proposal → 全建 → 刪 specs）

## WP2 — `spectra new artifact`（capability: `artifact-scaffold`）

- [ ] 2.1 probe 補齊：design/tasks/spec 三型的內容驗證規則（strings mining
      + 實測），回填 design.md
- [ ] 2.2 `spectra-core::artifact`：路徑解析（spec → `specs/<cap>/spec.md`）、
      `--stdin`/空模板、`--force`、per-type 驗證
- [ ] 2.3 CLI `new artifact <TYPE> [CAPABILITY] [--change] [--stdin] [--force]
      [--json]`：compact 單行 JSON、錯誤字串逐字對齊
- [ ] 2.4 整合測試：四型建檔 + 5 個錯誤案例（unknown type / 缺 capability /
      already exists / 驗證失敗 / change 不存在）

## WP3 — `spectra instructions`（capability: `artifact-instructions`）

- [ ] 3.1 probe 補齊：`unlocks` 語義、apply 模式 `state` 值域、`parallel`
      標記、preflight 判準、`--skill` 處置，回填 design.md
- [ ] 3.2 `spectra-core::instructions`：artifact 模式（模板組裝 +
      dependencies/unlocks）與 apply 模式（contextFiles/progress/tasks/
      preflight，複用 tasks.rs）
- [ ] 3.3 CLI `instructions [ARTIFACT] [--change] [--json]`：human + JSON
- [ ] 3.4 整合測試：4 artifacts × JSON keys、apply 模式進度／preflight

## WP4 — `spectra analyze`（capability: `change-analyze`）

- [ ] 4.1 probe 補齊：10 種 finding 的觸發條件／severity／措辭（每種正反
      fixture 實測），回填 design.md
- [ ] 4.2 `spectra-core::analyze`：4 維度 findings engine + snake_case 報告
      （含 `summary_msg`/`recommendation_msg` i18n key 結構）
- [ ] 4.3 CLI `analyze [CHANGE] [--json]`：human + JSON，恆 exit 0
- [ ] 4.4 整合測試：insufficient-artifacts skip 路徑 + 每種 finding 至少
      一正一反案例

## 收尾

- [ ] 5.1 RE 文件：`docs/reverse-engineering/artifact-workflow.md`（WP1–3）
      與 `analyze.md`（WP4）
- [ ] 5.2 README／CHANGELOG：新指令列表 + `new change` BREAKING 說明
- [ ] 5.3 sdd plugin 端 `--agent` 假設移除：於 yibi-stack 另開 issue（本 repo
      之外，僅記錄連結）
