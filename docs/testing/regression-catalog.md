# 回歸對照表：已回報並修正的缺陷 × mutation 驗證

這份文件回答一個問題：**過去回報並修正過的每個 bug，現在是否都有測試守住？**
「守住」的判準不是「有一個名稱相關的測試」，而是**把修正還原成當初的錯誤形狀，
指定的回歸測試會失敗**。每一列都對應 `scripts/mutations.toml` 裡的一個或多個
mutation case，由 `scripts/mutate-check.py` 實際執行驗證。

- 驗證日期：2026-09-27，base `7131cb4`（v0.12.0 之後的 main）
- 結果：`scripts/mutations.toml` 共 119 個 case，**119/119 KILLED**
- W9b（2026-09-28，validate 規則改依 OpenSpec 1.13.2）：新增 16 個 `w9b-*` case，
  並把 `80-first-block-only`、`80-scenario-text-counts`、`35-rename-chain-single-step`
  改指向新的實作（`openspec_md.rs`、`validate.rs`）。這 19 個加上
  `35-archive-scenario-loss`、`43-no-metadata-fallback`、`44-validate-typo-as-change`
  共 22 個 case 實測 22/22 KILLED；其餘 case 未在這一輪重跑
- #183（2026-09-28，scenario loss 只回報一則）：新增 3 個 `183-*` case，只重跑這 3 個，
  **3/3 KILLED**
- W10（2026-09-28）：analyze 拆成 `crates/spectra-core/src/analyze/` 模組，改寫列 23、24 的
  case 位置並新增 24 個 `w10-*` case（列 76）；只重跑這 26 個 case，**26/26 KILLED**，
  其餘 case 未在此次重跑
- W7g（2026-09-28，schema validate／fork 對齊 oracle 3.0.0，owner 裁決 D11）：新增 19 個
  `w7g-*` case，逐一以 `--only` 實測 19/19 KILLED；其餘 case 未在這一輪重跑
- W9a（2026-09-28，validate 輸出格式，決策 D3／D12）：新增 23 個 `w9a-*` case
  （23/23 KILLED）；2026-09-29 依 owner 裁決 W9a-1／W9a-3（撞名取 change、
  `ITEM --all` 忽略 `--all`）再加 4 個（4/4 KILLED），`44-validate-typo-as-change` 的 anchor 改指向新的 item 解析；
  它與 W9b 那一輪的其他 21 個 case 在 W9a 分支上重跑，22/22 KILLED。
  另有 6 個 case 的 anchor 在 origin/main 上就已找不到（`155-created-sort-*`、
  `52a`／`52b`、`53a`／`53b`），不是 W9a 造成的，尚未處理
- W14（2026-09-28，owner 裁決 D9／D10）：新增 `w14-d9-new-project-default`（新專案預設
  改回 `openspec`）與 `w14-d10-openspec-only-keeps-openspec`（只有 `openspec/` 的專案改用
  新專案預設），實測 2/2 KILLED；其餘 case 未在這一輪重跑
- W14-b（2026-09-28，`init --force` 保留既有 `.spectra.yaml`）：新增
  `w14b-force-rewrites-spectra-yaml` 與 `w14b-force-ignores-configured-spec-dir`，實測 2/2 KILLED
- #226（2026-09-29，`schemas` 列出載入失敗的 schema）：新增 `226-schemas-skip-unloadable`
  （改回「載入失敗就略過」）與 `226-schemas-require-schema-yaml-file`（改回要求
  `schema.yaml` 是檔案），逐一以 `--only` 實測 2/2 KILLED；其餘 case 未在這一輪重跑。
  同日依 owner 裁決（與內建同名的專案 schema 跟 oracle 一樣嚴格載入）再加
  `226-shadow-load-failure-skipped`、`226-shadow-appended-after-builtins`、
  `226-shadow-description-dropped`，`226-schemas-skip-unloadable` 的 anchor 改指向新的
  內建名稱判斷；五個 #226 case 逐一 `--only` 重跑 5/5 KILLED
- 來源：closed/open issue、merged PR 的 Review Contract 與 mob review 紀錄、
  `CHANGELOG.md` 的 Fixed 段、`git log` 的 fix commit、`docs/reverse-engineering/*.md`

## 為什麼要兩種 mutation

| 工具 | 突變什麼 | 抓得到什麼 |
|---|---|---|
| `cargo mutants` | 運算子、回傳值、match arm、`!`／`&&` | 邏輯分支沒有被任何測試觀察到 |
| `scripts/mutate-check.py` | 指定的字串替換，重現**當初的 bug 形狀** | 某個已修正的 bug 被改回去時，回歸測試會不會紅 |

cargo-mutants 不突變字串字面值與參數值，而本 repo 大多數已回報的 bug 正好屬於
這類：`--git-common-dir` 寫成 `--git-dir`、stop-list 少一個詞、`0o644` 寫死、
`truncate(50)` 取代取樣、`-o` grep 旗標吃掉重疊 needle。所以兩者互補，缺一不可。

## 怎麼跑

```sh
scripts/mutate-check.py                  # 跑全部 case（每個 case 一次增量編譯加測試）
scripts/mutate-check.py --only 118-common-dir
scripts/mutate-check.py --cases <other.toml>
```

每個 case 只改一件事；harness 會斷言 anchor 在檔案中**恰好出現一次**，並把以下
狀況都判為失敗（exit 1），不會靜默略過：anchor 找不到或重複、mutant 無法編譯、
測試過濾器沒選到任何測試、測試在 mutant 下仍通過（SURVIVED）、超過 900 秒。
每個 case 結束後都會還原原檔並 `touch`，避免 cargo 以 mtime 判斷而沿用 mutant
產物（見 `CLAUDE.md` 的 PR #84 事故）。harness 會直接改 worktree 內的檔案，
**執行期間不要編輯原始碼或測試**，也不要與另一個 cargo 程序並行。

harness 本身的負向對照（2026-09-27 實測）：選錯測試得到 SURVIVED、anchor 不存在
得到 `[FAIL] anchor 命中 0 次`、過濾器選不到測試得到 `[FAIL] 沒有選中任何測試`，
三者都 exit 1。

新增一個修正時，請在同一個 PR 補上對應的 case：`find` 取修正後的程式碼片段，
`replace` 寫回修正前的錯誤形狀，`test` 指向釘住它的回歸測試。

## 這一輪發現並補上的缺口

以下各項都先實測 mutant 會存活，補上測試後再實測會被殺掉。

1. **#118 linked worktree（原本沒有任何 `git worktree add` 測試）**：
   把 `--git-common-dir` 改成 `--git-dir`，所有既有測試都通過；catalog 當初列為
   pinned 的 park 測試全都經由 production 的 `parked_root()` 取路徑，所以儲存位置
   改到哪裡都看不出來。新增
   `change::tests::parking_from_a_linked_worktree_lands_in_the_shared_store`，
   直接寫出 oracle 的路徑 `.git/spectra-app/changes/<name>`。
2. **#155-2 archive 在終端機上的確認提示**：
   - 接受路徑（`y`／`Y`）原本沒有測試。新增 `archive_accepts_y_and_capital_y_on_a_terminal`。
   - 既有的 `archive_prompts_and_aborts_on_a_terminal` 是**空轉的**：寫完 `n\n` 立刻
     關閉 stdin，`script` 會對 PTY 送出 `^D`，程式先讀到 EOF 就 Aborted。實測把
     `n` 當成接受的 mutant 在舊版測試下存活。改用共用的 `run_on_terminal`，stdin
     撐到子程序結束。
3. **#6 `--no-color` 接線**：既有測試都跑在非 TTY，本來就不上色，把
   `color_enabled(cli.no_color)` 改成 `color_enabled(false)` 不會被發現。新增
   `no_color_flag_strips_ansi_codes_on_a_terminal`，並先斷言沒加旗標時確實有 ANSI
   碼（正向對照）。
4. **#155-1 `--sort created` 的 mtime fallback**：APFS 與 ext4 都有 birth time，整合
   測試永遠走不到 fallback。把時間戳選擇抽成純函式 `change::sort_timestamp`
   （不改行為），新增
   `created_sort_falls_back_to_mtime_only_when_birth_time_is_unavailable`。
5. **#160-4 已同步的 MODIFIED**：拿掉「內容相同就略過」的 `continue`，重建出來的
   spec 逐位元組相同，只有 `Specs applied: ... modified: N` 的計數會變，既有測試只比
   檔案內容所以守不住。在 `archive_treats_identical_added_and_modified_requirements_as_already_synced`
   補上計數斷言。
6. **#80 的語義變更（不是回歸）**：PR #82 把 SHALL/MUST 限縮到第一個文字區塊，
   PR #161 移除了這段。查證 OpenSpec 1.12.0 的 `dist/core/parsers/requirement-text.js`，
   上游自己把 `extractRequirementText` 改成 `extractRequirementBody`（讀取第一個 header
   之前的整段 body），所以 #161 是刻意跟進，不是意外回歸。新增兩個測試釘住 1.12
   的邊界：Goal-first 寫法會通過；SHALL 只出現在 scenario 內則不算。
7. **#159（oracle 的 bug）**：新增守護測試
   `archive_ignores_same_named_change_copies_inside_worktrees`，確保 worktree 內的同名
   change 副本不會阻擋 archive、也不會被動到。這一項沒有產品碼可以突變，只有守護測試。

## 需要人裁決的事項

- （已處理，W9b）決策 D1 讓 validate 依 OpenSpec 1.13.2：同一個 delta 檔已有 ERROR
  時不再以 archive 的措辭重報（OpenSpec `alreadyReported`），scenario 遺失只報一則，
  由 `w9b-c3-rereport-reported-delta` 守住。#183 的其餘驗收條件（2026-09-28）：
  `validate_rejects_a_modified_requirement_that_drops_a_current_scenario` 與
  `validate_follows_a_transitive_rename_chain_when_checking_scenario_loss` 改為斷言
  scenario loss **恰好一則**、`ERROR`、path `auth/spec.md`、validate 自己的措辭，且沒有
  `Archive would refuse` 重報；新增 `183-validate-scenario-loss-off`（兩個測試各一）與
  `183-archive-rereport`，3/3 KILLED。對照：舊版測試下 `183-archive-rereport` **存活**，
  證明新斷言才是守住去重的關鍵。D1 之後不再帶行號（OpenSpec 的 delta finding 多數沒有
  `line`），所以「保留帶行號的那則」不適用。以下保留原始紀錄。
- **`validate` 對 scenario 遺失回報兩次**（row 35）。`validate.rs` 自己的檢查與
  `archive::validate_archive_compatibility` 會對同一個缺陷各報一則 ERROR：前者帶
  `specs/<cap>/spec.md` 路徑與行號，後者只有 `changes/<name>`。實測拿掉前者後，兩個
  crate 的**全部測試**都仍然通過，所以它目前沒有任何測試守住。可行的做法有三：
  刪掉重複的一則、保留帶行號的一則並加測試釘住、維持現狀。這屬於輸出契約的設計
  決定，這一輪沒有替你選，也沒有寫測試把目前的重複行為固化。追蹤於 #183，
  該 issue 列出裁決後要補齊的測試與 mutation case。

## 已知的弱點（本輪未處理）

除非另外註明，以下是讀碼判斷，沒有做 mutation 實測。

- **row 6，`grep_existing` 的空 needle 提早 return**：守的是「不啟動 git 程序」，拿掉
  提早 return 後，`git grep` 在沒有 pattern 時走錯誤分支，同樣回傳空集合，輸出
  不變，值層級的 mutation 看不出來。
- **row 60，completion 的 symlink 暫存檔測試**：在 process 全域 `COUNTER` 猜測的序號
  0..63 預先放 symlink。單獨執行時有效，但在完整並行的 `cargo test` 下保證會變弱；
  `fsutil` 已改成注入暫存路徑，這裡還沒改。
- **row 62，`write_atomically_creates_a_missing_target_*`**：在 umask 077 的機器上會
  變成恆真；`fsutil.rs` 裡該測試的註解已把這點列為接受的殘餘盲點。umask 的覆蓋另外由 `umask_create_integration`
  的 4 種 umask 與 `read_umask_reports_the_mask_actually_in_effect_and_restores_it`
  （0o057 探針）負責。
- **row 31，非 UTF-8 capability 目錄名**：測試只在 `#[cfg(target_os = "linux")]` 下
  編譯。macOS 上跑 harness 會回報沒選到測試，所以沒有收進 case，只有 Linux CI 守得住。
- **row 74，#162（開放中）**：EXDEV fallback 的延後 finding 需要可注入錯誤的檔案系統
  介面才能測。

## catalog 引用錯測試的列

以下幾列確實有守住，只是當初整理時引用的測試不是真正守住它的那個。下表已改為
實測會殺掉 mutant 的測試。

- row 46／47：真正守住 oracle 儲存路徑的是新加的 worktree 測試；舊的 park 測試守不住。
- row 54：`list --specs` 的接線由 `cli_integration::list_and_show_support_nested_canonical_spec_ids`
  守住；`main.rs::list_specs_items_shape_*` 只測純函式，碰不到旗標。
- row 62：「mask 0o057 殺掉寫死 022 的 mutant」的是
  `read_umask_reports_the_mask_actually_in_effect_and_restores_it`，不是
  `newly_created_file_mode_uses_a_0666_base_filtered_by_umask`。
- row 25：修正在 PR #52（temp 檔加 rename），PR #54 只補了測試。

## cargo-mutants 全量結果（spectra-core）

2026-09-27 對 base `7131cb4` 執行（這一輪新增的測試尚未計入），`--test-workspace=true`，
也就是 core 的 mutant 用兩個 crate 的全部測試判定。為了降低機器負載分三階段跑：
先只跑 core 的測試篩選，再用較長的 timeout 重判 timeout，最後以 `--iterate`
把 missed 交給整個 workspace 的測試重判。

| 結果 | 數量 |
|---|---|
| caught | 1103 |
| unviable（無法編譯） | 111 |
| missed | 240 |
| timeout（`+=` 改成 `*=` 之類的無窮迴圈） | 3 |

扣除 unviable 後 mutation score 約 82%。missed 清單完整保存在
`docs/testing/cargo-mutants-missed.txt`，依檔案分布如下：

- `search.rs` 44、`schema.rs` 36、`archive.rs` 36、`validate.rs` 17、`spec.rs` 15、
  `spec_diff.rs` 15、`markdown.rs` 13、其餘各檔 11 以下。
- 多數不在已回報 bug 的範圍內（例如 `spectra search` 佔了 32 個）。

落在已修正 bug 範圍內、值得優先補測試的：

- **`archive.rs::rollback_prepared_specs`（#160-3）**：5 個 guard，都是還原時
  「目前內容已等於原始內容就跳過」與「原本不存在」的分支。主要的 rollback 行為
  已由 `37-*` 的 case 驗證，這些細部分支還沒有測試觀察到。
- **`archive.rs` 的 `NotFound` guard**（`restore_file`、`read_optional_bytes`、
  `has_any_file`、`path_entry_exists_io`）：要能注入 I/O 錯誤才測得到，和 row 74
  的 #162 同一類。
- **`schema.rs::yaml_inline_comment_start`（12 個）**：與 #160-7 的「fork 只改頂層
  `name`、保留 YAML 格式」相鄰，現有 fork 測試沒有涵蓋行內註解的各種形狀。
- **`archive.rs:98`（#142 `archive --no-validate`）**：`!skip_specs && !no_validate`
  的突變存活。原因很可能是後面的 `prepare_spec_deltas` 會對同一批錯誤報錯，所以是
  等價 mutant，還沒查證。

重跑建議用溫和的設定，避免拖垮機器：

```sh
CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=2 cargo mutants --package spectra-core \
  --test-workspace=true --timeout 600 -j 2 -o <output-dir>
```

## 對照表

欄位「mutation case」列出 `scripts/mutations.toml` 裡的 case id，全部實測 KILLED。
`—` 表示該列不適用（CI YAML、已移除的功能、純測試碼）或仍開著。

| # | ID | 症狀 | mutation case（全部 KILLED） |
|---|---|---|---|
| 1 | PR #2 | Structure 分數只用 decay 階梯計算；3/40 個 FilePath broken 回傳 3（oracle 回傳 0） | `01-structure-category-weight` |
| 2 | #10 / PR #35 | Time 維度差一天：第 21 天被算成 stale、第 60 天被算成 abandoned；abandoned 分數是 3，oracle 是 4 | `02-time-aging-stale-edge`<br>`02-time-stale-abandoned-edge`<br>`02-time-abandoned-score` |
| 3 | #37 / PR #38 | `drift` 在 medium 以上嚴重度時 exit 非 0 | `03-drift-exit-code-severity-map` |
| 4 | #12 / PR #25 | 批次 grep 若用 `-o`，會吃掉重疊的 needle，誤報 broken | `04-grep-only-matching` |
| 5 | PR #25 後續 | 使用者設定 `color.grep=always` 時，ANSI 色碼混進輸出，所有 needle 都被誤判 | `05-grep-no-color` |
| 6 | PR #25 | git grep 出錯（exit >1 或 spawn 失敗）時，必須把所有 needle 當成已解析，不能全部報 broken | `06-grep-error-treated-as-resolved` |
| 7 | #119 / PR #124 | `ANCHOR_CAP` 實作成 `truncate(50)`，分母固定是 50，第 50 個之後的 broken 全部遺失 | `07-anchor-cap-truncate` |
| 8 | #119 C02 / PR #124 | CliFlag 與找不到的 Function 被分類成 unresolved（oracle 報 broken；這是 revert #83 其中兩類的結果） | `08-cliflag-unresolved`<br>`08-function-unresolved` |
| 9 | #83 / PR #104 | change 自己要新建的 FilePath 被報成 broken（真實 repo 的訊號率只有 8%）；改成 baseline 時不存在的回報 `forward reference` unresolved，且不計分 | `09-forward-reference`<br>`09-forward-reference-needs-usable-baseline` |
| 10 | #123 / PR #130 | FilePath 錨點被剝掉第一個路徑段，回報一個 design 原文裡不存在的字串；中段比對會產生幽靈錨點（`mysrc/foo.rs` 變成 `src/foo.rs`） | `10-filepath-leading-segments`<br>`10-filepath-left-boundary` |
| 11 | PR #130 review | 含 `..` 的路徑可以用專案外的檔案解析成功 | `11-filepath-dotdot-guard` |
| 12 | PR #130 | 巢狀根目錄：專案根是 `frontend/` 時，design 引用 `frontend/src/x.ts` 必須靠截斷後的形式解析 | `12-path-candidates-union` |
| 13 | PR #135 | baseline probe 必須和現況 probe 使用同一個聯集（先前聲稱「聯集不可能製造 broken」是錯的） | `13-baseline-probe-union` |
| 14 | #51 / PR #130 | 全新 scaffold 的 design 多出一個 `JSON` Symbol（oracle 是 0/20） | `14-stoplist-json` |
| 15 | #133 / PR #137 | stop-list 少了 20 個 oracle 會丟掉的詞 | `15-stoplist-133-sweep` |
| 16 | PR #54（v0.4.0） | `- [ ] ` 這種空描述 checkbox 被 `tasks.rs` 計入，但 apply 不計入，於是 `task done <id>` 勾到錯的那一行 | `16-tasks-empty-description`<br>`16-apply-empty-description` |
| 17 | #172 / PR #177 | `tasks.rs` 只認 `-` bullet；`*`/`+` bullet 與 `[~]` marker 的編號和 apply 不一致，勾錯行 | `17-checkbox-bullet-styles`<br>`17-checkbox-any-marker`<br>`17-done-marker-only-x`<br>`17-mark-done-other-marker-untouched` |
| 18 | PR #19 → PR #23 | `task done` 把 `.spectra/` 自己的狀態檔記進 touched | `18-touched-excludes-state-dir` |
| 19 | #98 D / PR #173 | change 開始前就 dirty、之後沒被任何 task 改過的檔案，被記進 touched | `19-touched-baseline-fingerprint`<br>`19-touched-now-clean-candidates` |
| 20 | PR #173 | archive 的 `@trace` `code:` 列出已經刪掉的路徑 | `20-trace-omits-missing-paths`<br>`20-trace-not-a-directory-is-gone`<br>`20-trace-keeps-dangling-symlink`<br>`20-archive-clears-baseline-sidecar` |
| 21 | PR #19 review | `git status` 的 rename 與 `->` 解析把含箭頭的檔名、含空白或非 ASCII 的檔名弄壞 | `21-dirty-files-nul-delimited`<br>`21-dirty-files-rename-old-path` |
| 22 | review | 損壞的 touched JSON 被靜默丟掉 | `22-touched-corrupt-backup`<br>`22-touched-corrupt-backup-no-clobber` |
| 23 | PR #54 | 弱語言清單只有 5 個詞，oracle 有 9 個（缺 consider/possibly/TODO/TKTK） | `23-weak-language-nine-words` |
| 24 | PR #54 | analyze 掃描 `specs/` 下所有 `*.md`，`notes.md` 這類附檔產生幽靈 finding | `24-analyze-only-spec-md` |
| 25 | PR #54 | `new artifact --force` 可以透過預先放好的 symlink 寫到 change 目錄外 | `25-artifact-force-no-symlink-follow` |
| 26 | PR #48 → PR #52 | git identity 含 YAML 特殊字元時，`created_by` 以原始字串串接，寫出無法解析的 `.openspec.yaml` | `26-created-by-yaml-quoting`<br>`26-archived-by-yaml-quoting` |
| 27 | PR #52 | artifact 建立有 TOCTOU：並行建立時靜默覆寫；寫到一半的檔案被當成 done | `27-artifact-no-clobber-install` |
| 28 | PR #52 | specs done-check 不跟隨目錄 symlink；I/O 錯誤被折成「not done」 | `28-specs-done-follows-dir-symlink`<br>`28-specs-io-error-not-not-done`<br>`28-specs-symlink-cycle-terminates` |
| 29 | #39 / PR #41 | archive 只走一層，巢狀 `specs/<Epic>/<Feature>/spec.md` 的 delta 被靜默忽略 | `29-one-level-walk` |
| 30 | PR #41 | `specs/` 下的 symlink 迴圈會讓遞迴 stack overflow | `30-follow-dir-symlinks` |
| 31 | PR #41 | 非 UTF-8 的 capability 目錄名以 lossy 方式轉換，archive 寫到錯的路徑；`specs/spec.md` 直接放在根下被靜默處理 | `31-orphan-root-spec` |
| 32 | PR #32（Gemini P1） | RENAME 的目標名和既有 requirement 衝突，讓後續 MODIFY/REMOVE 作用在錯的 requirement | `32-rename-onto-existing-target`<br>`32-rename-from-unnormalized`<br>`32-duplicate-unnormalized` |
| 33 | review | MODIFIED 黏行：`trim_end` 過的替換內容把下一個 `### Requirement:` 黏到前一行尾，該 requirement 被靜默丟掉 | `33-modified-glue` |
| 34 | review | canonical spec 沒有 `## Requirements` 標題時，退回盲目附加到檔尾 | `34-blind-eof-append` |
| 35 | #160-1 / PR #161 | MODIFIED 只要還有一個 scenario 就通過，archive 會把其他 scenario 刪掉 | `35-archive-scenario-loss`<br>`35-rename-chain-single-step`<br>`183-validate-scenario-loss-off`<br>`183-validate-scenario-loss-off-rename-chain`<br>`183-archive-rereport` |
| 36 | #160-2 | fenced code block 內的 `### Requirement:` 被當成真的 requirement | `36-requirement-header-unmasked`<br>`36-section-end-unmasked`<br>`36-fence-indent-unbounded` |
| 37 | #160-3 | archive 先移動 change 再寫 spec；寫入失敗時留下一半已 archive 的狀態 | `37-no-change-restore`<br>`37-no-spec-rollback`<br>`37-rollback-clobbers-concurrent-spec`<br>`37-metadata-rollback-clobbers-concurrent` |
| 38 | #160-4 | 先 sync 再 archive 會失敗（已同步的 operation 被拒絕） | `160-4-identical-modified-counted`<br>`38-added-already-synced-rejected`<br>`38-rename-already-synced-rejected`<br>`38-missing-remove-over-tolerated` |
| 39 | #160-5 | 巢狀的 canonical spec 在 `list --specs` 與 `show` 看不到 | `39-list-specs-one-level`<br>`39-show-single-component-id` |
| 40 | #160-6 | 新 capability 已撰寫的 Purpose 被 TBD 取代 | `40-purpose-replaced-by-tbd` |
| 41 | #160-7 | `schema fork` 後的 schema 仍以來源名稱（`spec-driven`）自稱 | `41-fork-keeps-source-name`<br>`41-fork-rewrites-nested-name` |
| 42 | v0.12.0 Fixed | 重建後的 spec 結尾換行數不一致 | `42-final-newline-not-canonicalized` |
| 43 | #80 / PR #82 | SHALL/MUST 掃描整個 body，Goal-first 的 requirement 因此通過（OSS 1.5.0 會拒絕） | `80-first-block-only`<br>`80-scenario-text-counts`<br>`43-no-metadata-fallback` |
| 43b | W9b（D1） | validate 規則與 OpenSpec 1.13.2 不同：archive 衝突報 ERROR、stray `###` 吃掉 scenario、空 scenario 算數、重複 section 被拒、`specs/spec.md` 中止整批、main spec 只認 `### Requirement:`、Purpose 只找 `##`、沒有結構檢查與 Purpose 長度檢查、長度用 UTF-8 位元組、SHALL 用 Unicode 字界、task 編號不看 schema 來源 | `w9b-c1-archive-refusal-as-error`<br>`w9b-c3-rereport-reported-delta`<br>`w9b-c20-keeps-every-archive-error`<br>`w9b-c20-drops-openspectra-only`<br>`w9b-c4-stray-h3-ends-block`<br>`w9b-c5-empty-scenario-counts`<br>`w9b-c7-first-section-only`<br>`w9b-c9-no-missing-header-error`<br>`w9b-c11-root-spec-ignored`<br>`w9b-c18-gate-ignores-source`<br>`w9b-s1-only-requirement-children`<br>`w9b-s2-no-structure-check`<br>`w9b-s7-purpose-level-two-only`<br>`w9b-s9-no-brevity-warning`<br>`w9b-utf8-byte-length`<br>`w9b-unicode-word-boundary` |
| 43c | W7g（D11） | `schema validate`／`fork` 與 oracle 3.0.0 不同：不帶名稱時驗全部專案 schema、輸出形狀與串流、serde 錯誤原文被吃掉、不要求 version、要求 apply／instruction、cycle 先於 apply 檢查、`schema.yaml` 是目錄時當成找不到、缺檔 template 被判無效、內建 fork 的 artifact 順序、空字串與 `.` 目標外洩 stage 目錄、`--force` 蓋一般檔案時外洩 backup、fork 的 ✓ 沒上色；以及 D11-1／D11-7 的路徑拒絕（刻意分歧） | `w7g-validate-default-spec-driven`<br>`w7g-validate-failure-on-stderr`<br>`w7g-version-required`<br>`w7g-parse-error-cause`<br>`w7g-apply-requires-check`<br>`w7g-schema-yaml-exists`<br>`w7g-no-apply-requires-all`<br>`w7g-instruction-key-omitted`<br>`w7g-instruction-section-omitted`<br>`w7g-template-section-omitted`<br>`w7g-template-warning-not-error`<br>`w7g-template-warning-empty`<br>`w7g-template-escape-rejected`<br>`w7g-generates-escape-rejected`<br>`w7g-builtin-fork-order`<br>`w7g-fork-empty-target`<br>`w7g-fork-dot-target`<br>`w7g-fork-backup-file-removed`<br>`w7g-fork-check-mark-color` |
| 43d | W9a（D3、D12） | validate 的輸出格式：oracle 格式把 INFO 全顯示或全丟掉、delta 檔訊息沒加（或每條都加）`specs/<path>: `、`--all`／`--changes --specs`／不帶參數的 scope 退回 oracle 或空集合、不接受 spec 名稱、change 依名稱排序、warn 先於 error、少了 `Error: Validation failed.`、顏色錯、findings 只看 errors、`--format` 不需要 `--json`、撞名時 oracle 格式回 ambiguous（或 openspec 格式取 change）、`ITEM --all` 被拒或驗全部；openspec 格式依 byte 排序、大小寫不當 tie-break、`byType` 只列有 item 的 type、`totals`／issue 的 key 順序錯、findings 的 scope 錯、建議數錯 | `w9a-every-info-shown`<br>`w9a-archive-refusal-dropped`<br>`w9a-delta-prefix-missing`<br>`w9a-prefix-every-message`<br>`w9a-root-spec-not-delta`<br>`w9a-unread-delta-not-delta`<br>`w9a-all-changes-only`<br>`w9a-changes-specs-specs-only`<br>`w9a-bare-validate-empty`<br>`w9a-spec-name-rejected`<br>`w9a-oracle-change-order-by-name`<br>`w9a-warnings-before-errors`<br>`w9a-no-validation-failed-line`<br>`w9a-valid-glyph-red`<br>`w9a-findings-ignore-warnings`<br>`w9a-format-without-json`<br>`w9a-openspec-byte-order`<br>`w9a-locale-case-tiebreak`<br>`w9a-bytype-present-only`<br>`w9a-totals-key-order`<br>`w9a-issue-line-after-message`<br>`w9a-findings-scope-both-flags`<br>`w9a-suggestion-count`<br>`w9a-oracle-collision-ambiguous`<br>`w9a-openspec-collision-takes-change`<br>`w9a-item-all-not-ignored`<br>`w9a-item-all-rejected` |
| 44 | mob review | `validate <打錯的名稱>` 回報令人誤解的「no delta」，而不是 `Change 'x' not found.` | `44-validate-typo-as-change` |
| 45 | #134 / PR #138 | 沒有 active change 時，`status`/`instructions`/`drift`/`analyze` exit 1（oracle exit 0） | `45a-no-active-read-cmd-exits-1`<br>`45b-resolve-optional-empty-is-error` |
| 46 | #118 / PR #125 | park 寫一個 `.parked` 標記，而不是把目錄移進 `<git common dir>/spectra-app/changes/`；與 oracle 互相看不到 | `46b-parked-store-location-literal-path` |
| 47 | #118（worktree 那一半） | 從 linked worktree park 時，必須落在**共用**的 git dir，而不是 `.git/worktrees/<n>` | `118-common-dir` |
| 48 | PR #125（刻意偏離 oracle） | park 同名時覆寫既有的 parked change；`park archive` 會吞掉整個 `changes/archive/` | `48a-park-clobbers-existing-parked`<br>`48b-park-swallows-archive` |
| 49 | #117 / PR #128 | 自訂 `schema:` 靜默退回內建的 spec-driven | `49a-config-schema-ignored-falls-back-to-builtin`<br>`49b-change-level-schema-ignored` |
| 50 | #117 | `new change` 寫死 `schema: spec-driven`，讓上面那道 gate 永遠碰不到 | `50-new-change-hardcodes-spec-driven` |
| 51 | #126 / PR #150 | 自訂 schema 的 `schema.yaml` 從來沒有被載入 | `51-custom-schema-yaml-never-loaded` |
| 52 | #127 / PR #136 | `instructions --json` 缺少 config.yaml 的 `context` 與 `rules` | `52a-instructions-json-drops-context`<br>`52b-instructions-json-drops-rules` |
| 53 | #88 / PR #154 | `list --json` 多了 `summary` 欄位；人類可讀格式和 oracle 的 `Changes:` 加 bullet 不同 | `53a-list-json-readds-summary`<br>`53b-list-human-drops-bullets` |
| 54 | #3 / PR #13 | `list --specs` 是沒有作用的旗標 | `54-list-specs-flag-inert` |
| 55 | #4 / PR #14 | `list --parked` 是沒有作用的旗標 | `55-list-parked-flag-inert` |
| 56 | #6 / PR #16 | 全域 `--no-color` 是沒有作用的旗標 | `56-no-color-flag-inert` |
| 57 | #155-2 | archive 在 TTY 下的確認：接受（`y`）的路徑沒有測試 | `155-tty-capital-y-rejected`<br>`155-tty-n-accepted`<br>`57-archive-tty-lowercase-y-rejected` |
| 58 | #155-1 | `--sort created` 在沒有 birthtime 的檔案系統上退回 mtime | `155-created-sort-no-mtime-fallback`<br>`155-created-sort-prefers-mtime` |
| 59 | #90 | init 的原子寫入用 `fs::write` 建立暫存檔，會跟隨預先放好的 symlink（`.spectra.yaml.tmp-<pid>-0`） | `59a-temp-file-follows-symlink`<br>`59b-init-gitignore-plain-write` |
| 60 | PR #87 R2 | completion 的暫存檔穿過 symlink 寫入 | `60-completion-temp-follows-symlink` |
| 61 | PR #87 | `XDG_*`/`HOME` 為空或是相對路徑時，把 completion 寫到 cwd | `61a-completion-accepts-empty-relative-xdg-unit` |
| 62 | #93 / PR #100 | 新建的檔案權限是 0600，oracle 是 `0666 & ~umask` | `62a-new-file-mode-0600`<br>`62b-hardcoded-umask-022`<br>`62c-new-file-mode-0600-cross-umask` |
| 63 | PR #100 review | 被替換的 symlink 目標會以放寬後的權限重建；既有檔案的權限位元遺失 | `63a-symlinked-target-widened`<br>`63b-existing-mode-not-preserved` |
| 64 | #94 / PR #101 | init 的預設產物與 oracle 有 6 處不同（沒有 `config.yaml`、沒有 `changes/archive/`、`.spectra.yaml` 只有一行、第 6 行的 spec_dir、`.gitignore` 標頭、stdout） | `64a-init-no-changes-archive`<br>`64b-init-no-config-yaml`<br>`64c-init-spectra-yaml-one-line`<br>`64d-init-gitignore-no-header`<br>`64e-init-stdout-old-wording` |
| 65 | PR #86 review | managed block 的合併採行錨定整行替換，會刪掉使用者內容（空 body、END 同一行的尾隨內容） | `65a-managed-block-eats-start-line-prefix`<br>`65b-managed-block-eats-end-line-suffix` |
| 66 | PR #86 review | 既有檔案是 latin-1 時，`update` 整個中止（17 個檔只寫出 4 個） | `66-update-aborts-on-non-utf8` |
| 67 | PR #86 R2 | managed/settings 目標是目錄時，錯誤訊息多出一段絕對路徑 | `67-read-existing-adds-path-context` |
| 68 | PR #84 | 全域 config 含非 UTF-8 內容時被歸類成 I/O 錯誤，拒絕寫入 | `68-global-config-non-utf8-is-io-error` |
| 69 | #167 / PR #171 | 未知的 tool id 被靜默忽略 | `69a-unknown-tool-silently-ignored` |
| 70 | #92 / PR #102 | `claude_slash_commands: true` 時靜默不做事 | — |
| 71 | #148 / PR #151 | Docker 推送到寫死的 `ghcr.io/howie`，在 heyu-ai org 下被拒絕 | — |
| 72 | PR #34 | musl 的 static-link 檢查拒絕 x86_64 的 `static-pie linked` | — |
| 73 | PR #121 | sha2 0.11 沒有 `LowerHex`，測試編譯失敗 | — |
| 74 | #162（開放中） | archive 的 EXDEV fallback：刪除來源時部分失敗、來源沒有凍結、delta 來源在準備後被改動 | — |
| 75 | #159（開放中） | oracle 的 archive 在 `.claude/worktrees/` 裡有同名 change 副本時拒絕執行 | 無產品碼可突變；守護測試 `archive_ignores_same_named_change_copies_inside_worktrees` |
| 76 | W10 | `analyze` 與 oracle 3.0.0 不一致：少了 Localization 維度與三種新 finding、specs 判定吃進巢狀檔、capability／task／design topic／具體資料的比對規則是 2.3.1 版，findings 沒有依種類分組 | `w10-specs-presence-one-level`<br>`w10-concrete-data-given`<br>`w10-scenario-block-ends-at-h4`<br>`w10-no-scenario-skips-removed`<br>`w10-requirement-block-to-next-requirement`<br>`w10-weak-language-skips-headings`<br>`w10-design-topic-token-coverage`<br>`w10-design-topic-numbering-prefix`<br>`w10-goals-overlap-forty-percent`<br>`w10-capability-token-without-spaces`<br>`w10-capability-every-section-line`<br>`w10-missing-task-task-lines-only`<br>`w10-missing-task-skips-removed`<br>`w10-renamed-from-name-checked`<br>`w10-repeated-section-replaces`<br>`w10-purpose-placeholder-case-sensitive`<br>`w10-new-capability-empty-purpose`<br>`w10-localization-letter-floor`<br>`w10-localization-findings-first`<br>`w10-numeric-same-number-index`<br>`w10-numeric-equal-values-first`<br>`w10-numeric-transition-source-structural`<br>`w10-analyze-multi-change-wording`<br>`w10-analyze-colors-on-a-terminal` |
| 77 | #219（W12-3） | 日期開頭的使用中 change（`changes/2026-05-05-foo/`）被 `list_active` 當成封存 change 濾掉：`list`、`validate`、`status --all` 看不到它，不指定 change 的指令也不會自動選到它 | `219-dated-active-filtered`<br>`219-dated-active-not-auto-selected` |
