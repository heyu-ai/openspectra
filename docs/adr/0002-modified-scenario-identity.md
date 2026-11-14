# ADR-0002：MODIFIED 區塊的 scenario 身分與刻意移除（刻意與 OpenSpec 1.13.2 分歧）

- 日期：2026-11-14
- 狀態：**Proposed**
- 決策者：（待填）
- 關係：修改 D1（`validate` 規則以 OpenSpec 1.13.2 為準）的適用範圍；裁決紀錄列為 D13（`docs/migration-plan.md`）。

本 ADR 由 agent 起草。owner 在 yibi-mvp 的 session 中選定「讓 openspectra 與 OpenSpec 分歧、改用 slug 比對」這個方向（2026-11-14）。正式核准、填寫決策者與調整下列細節，留給 owner。

## 1. 問題

OpenSpec 1.13.2 以 scenario 標頭整行（`#### Scenario:` 之後的文字）當身分。`validate` 的兩處檢查與 `archive` 都用這個規則判定 MODIFIED 區塊有沒有遺漏 scenario（`validator.js:578-655`，本 repo 的移植在 `openspec_md::diff_scenario_names` 與 `archive::missing_scenarios_in_modified`）。

yibi-mvp 的 scenario 標頭格式是 `<slug> -- <可讀標題>`，slug 才是身分（yibi-mvp ADR-0008，test docstring 以 `spec: <cap>#<slug>` 引用）。在這個慣例下，整行比對有三種誤判：

1. 只改標題、slug 不變，被判成「刪掉一個、新增一個」。
2. 為沒有 slug 的舊 scenario 補上 slug，被判成遺失。
3. 依產品裁決刻意刪除 scenario 時，MODIFIED 區塊沒有任何寫法能通過；唯一的路是把整個 requirement 改成 REMOVED 加 ADDED。

實測（2026-11-14，yibi-mvp `validate --all --strict --format openspec`）：41 個 active change 中 7 個因此被判無效，成為每個 PR 留言 baseline 段的固定雜訊。其中 0196 的刪除是 proposal 明文要求的。

另一個事實：yibi-mvp 實際 archive 時用的是 shadow 模式的 Spectra 3.0.0 oracle，它會照套刪掉 scenario 的 MODIFIED 區塊（`simulate-archive-chain.py 0196` 實測：scenario 4172 → 4165）。所以 CI 的 gate 比實際 archive 嚴格。

## 2. 決策（草案）

1. **身分**：除了整行相同，下列情況也算同一個 scenario：
   - slug 相同，且這個 slug 在 current 與 MODIFIED 兩邊各自只出現一次。
   - current 沒有 slug，MODIFIED 補上 slug，且 ` -- ` 後的標題與 current 整行相同。
2. **刻意移除**：MODIFIED 區塊內的 `<!-- removed-scenario: <slug 或整行> -->` 只豁免它點名的那一個 scenario。
3. **沒用到的標記**報 INFO。不用 WARNING 的原因：oracle archive 會把標記原樣寫進 main spec，下一個從 main spec 抽出的 MODIFIED 會帶著它；WARNING 在 `--strict` 下會讓這種情況判失敗。
4. OSS `archive` 寫回 main spec 前拿掉標記行。
5. validate 的兩處檢查與 archive 共用 `scenario_identity.rs`，三者的判定才會一致。
6. 仍失敗時，訊息措辭不變。

## 3. 被否決或待 owner 確認的選項

| 選項 | 內容 | 狀態與理由 |
|---|---|---|
| 維持 D1，整行比對 | 不分歧，下游改用 REMOVED 加 ADDED | 下游（yibi-mvp）owner 已選分歧；維持 D1 需要改寫 7 個進行中的 change |
| 只做 slug 比對、不加移除標記 | 身分改 slug，刻意刪除仍然不行 | 實測只讓 1 個 change 變有效，其餘 6 個的錯誤是真的刪除或換掉 scenario |
| 無條件用 slug 比對 | 不檢查 slug 是否唯一 | 否決：`login -- error path` 換成 `login -- new path` 會被當成同一個 scenario，藏住真正的刪除 |
| 標記放在 requirement 區塊外 | 例如另開 `## REMOVED Scenarios` 區段 | 待確認：不會被 oracle archive 複製進 main spec，但 OpenSpec 與 oracle 對未知 `##` 區段的處理尚未實測 |
| 標記的語法 | 目前是 HTML 註解 `removed-scenario:` | 待確認：其他語法（例如帶 namespace 的 `openspectra:removed-scenario`）同樣可行 |
| 只在 `--format openspec` 以外套用 | 讓 openspec 格式維持逐欄等於 OpenSpec | 否決：yibi-mvp 的 CI 用的正是 `--format openspec`，這樣解決不了問題 |

## 4. 後果

- `--format openspec` 在上述三種形狀上不再與 OpenSpec 1.13.2 逐欄相同。golden replay 不受影響，因為它的 fixture 沒有這些形狀（`cargo test --all` 全綠）。
- oracle archive 會在 main spec 留下標記行。它在 main spec 沒有作用；被下一個 MODIFIED 帶走時只報 INFO。
- 標記讓「刻意刪除」在 delta 裡留下明文紀錄，reviewer 看得到。

## 5. 驗證

- `scenario_identity.rs` 單元測試 9 個、`scenario_identity_integration.rs` CLI 整合測試 5 個。每個整合測試都用同一份 fixture 同時跑 validate 與 archive。
- 6 個 `d13-*` mutation case 全部 KILLED；兩個因重構移位的既有 case（`160-4-identical-modified-counted`、`33-modified-glue`）重新定錨後也是 KILLED。
- yibi-mvp 實測（三個 binary 比較每個 item 的 strict 判定）：v0.14.2 與 `origin/main`（0.14.6）完全相同；本分支只影響 4 個 change——0157 變有效，0098 少 2 個 ERROR，0163 少 1 個，0070 的錯誤從誤判換成真正的刪除；56 個 stable spec 的判定不變。
