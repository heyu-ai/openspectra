# 逆向工程規格與探測紀錄

這裡保存遷移執行佇列（`docs/migration-plan.md`）中，由 RE agent 產出、但**尚未**或**剛**轉成正式
RE 文件的規格與探測腳本。原本只存在背景 job 的暫存目錄，job 刪除時會一起消失，
所以在 2026-09-28 搬進 repo。

| 目錄 | 工作項 | 狀態 | 正式 RE 文件 |
|---|---|---|---|
| `w7g-schema/` | W7g：`schema validate`／`schema fork` | D11 已裁決，待實作 | 實作時寫進 `docs/reverse-engineering/` 對應文件 |
| `w9-validate/` | W9：`validate` 規則與 oracle 格式 | W9b 已 merge（#214）；W9a 依 D12 待實作 | `docs/reverse-engineering/validate.md` |
| `w12-commands/` | W12：`decisions`／`demo`／`feedback`／`show` 旗標 | PR #216 | `decisions.md`、`demo-feedback.md`、`list-show.md` |

## 怎麼讀

- 每份 `SPEC.md` 的規則後面標有 `[V pNN]`（以探測腳本 `pNN_*` 實測）或 `[I]`（推論，未實測）。
  兩者的可信度不同，實作時以 `[V]` 為準，`[I]` 需要先補探測。
- `w7g-schema/golden/` 是 `schema fork` 的輸出擷取：`oracle/` 為 oracle 3.0.0、`ours/` 為當時的
  OpenSpectra，用來比對 artifact 順序與重新序列化的差異。
- 正式 RE 文件與 SPEC 衝突時，以正式 RE 文件為準；SPEC 是提出當時的紀錄，不會隨實作更新。

## 腳本不能直接重跑

只搬了文字產物（`*.md`、`*.sh`、`*.py`、`*.sb`、`*.json`、`*.summary`，單檔 200 KB 以下），
**jail、harvest、corpus 副本都沒有搬**。腳本裡寫死的路徑指向當時的環境：

- `/Users/howie/.claude/jobs/9eb90dff/tmp/...`：原本的暫存目錄（jail 與中間產物所在）
- `/Users/howie/Workspace/github/...`：當時的 repo 與 corpus 專案位置
- oracle binary 與 `~/.npm/_npx/...` 下的 OpenSpec 1.13.2

要重現時請參考腳本邏輯改寫路徑；可重複執行、有比對契約的版本是 `scripts/capture-*.py`。
