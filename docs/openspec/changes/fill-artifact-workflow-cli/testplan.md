# Testplan：fill-artifact-workflow-cli

> 版本：v1.0 | 日期：2026-07-18 | Effort：high
> 產出：主 session 依 qa-test-design 六技法（EP / BVA / DT / ST / PW / RB）整理；
> 所有 TC 均已在 PR #48 實作並通過（274 tests 全綠），另有 macOS 本機 oracle
> side-by-side 抽查（不進 CI，同 capture-golden.sh 慣例）。

## 1. Test Case Table

### capability: workflow-status（US-101，已實作）

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result |
|-------|-------------|-----------|------|-------------|-------|-----------|----------------|
| STAT-ST-001 | 新建 change 只有 proposal ready，其餘 blocked 且 missingDeps 正確 | ST | High (I4×L3) | init + new change，無 artifact 檔 | Rust `status_empty_change_reports_only_proposal_ready`（`crates/spectra-cli/tests/status_integration.rs`） | 空 change | proposal=ready；design/specs blocked by proposal；tasks blocked by specs（非 design）；isComplete=false |
| STAT-ST-002 | done 僅由檔案存在性決定、狀態不 cascade | ST | High (I4×L3) | 四 artifact 檔齊備後刪除 `specs/` | Rust `status_file_existence_does_not_cascade_after_specs_are_deleted` | 刪 specs 目錄 | specs 回 ready；tasks 維持 done；isComplete=false |
| STAT-VL-001 | 全 done 時無 missingDeps 欄位、附完成行 | VL | Medium (I3×L3) | 四 artifact 檔齊備 | Rust `status_complete_omits_missing_deps_and_prints_completion_line` | 完整 change | 全部 done；JSON 無 missingDeps key；human 結尾 `  ✓ All artifacts complete`；applyRequires=["tasks"] |
| STAT-VL-002 | camelCase JSON 與 human ✓/○/✗ 格式對齊 oracle | VL | High (I4×L3) | 任一 change | Rust `status_json_and_human_contracts_match_the_oracle` + 手動 oracle side-by-side（status human/JSON 逐位元，含正向對照） | 多狀態矩陣 | keys 為 camelCase；blocked 項下一行 `    blocked by: <deps>` |
| STAT-EP-001 | `new change` 只寫 3 個 metadata key、不 scaffold artifact（BREAKING） | EP | High (I5×L3) | init 後 new change | Rust `new_change_writes_only_the_three_oracle_metadata_keys` | 新 change | `.openspec.yaml` 恰含 schema/created/created_by；無 proposal.md/design.md/tasks.md |
| STAT-RB-001 | schema 常數與 oracle goldens 逐 byte pinning | RB | High (I5×L2) | goldens 已採集 | Rust `schema.rs` 單元測試（9 個，含 golden pinning：解析 `docs/reverse-engineering/golden/instructions-*-2.3.1.json` 與嵌入常數逐 byte 比對） | 4 artifacts | instruction/template 常數與 goldens 完全一致；DAG deps/outputPath 一致 |

### capability: artifact-scaffold（US-102，已實作）

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result |
|-------|-------------|-----------|------|-------------|-------|-----------|----------------|
| ART-VL-001 | `--stdin` 逐位元寫入 + 單行 compact JSON | VL | High (I4×L3) | change 無 proposal.md | Rust `proposal_stdin_writes_exact_bytes_and_compact_json`（`crates/spectra-cli/tests/artifact_integration.rs`） | 含 `## Why` 內容 | 檔案 byte 一致；stdout 單行 `{"artifact",...,"validated":true,"warnings":[]}` |
| ART-EP-001 | 模板建檔（無 --stdin）用 schema 常數且 validated=false | EP | High (I4×L3) | change 無 design.md | Rust `design_template_uses_schema_constant_and_is_not_validated` | 無 stdin | 檔案內容=schema template；JSON validated=false |
| ART-VL-002 | tasks checkbox 驗證通過 | VL | Medium (I3×L3) | change 無 tasks.md | Rust `tasks_stdin_with_checkbox_is_validated` | `- [ ]` 內容 | validated=true |
| ART-VL-003 | spec 落在 `specs/<cap>/spec.md` | VL | High (I4×L3) | change 無該 capability | Rust `spec_stdin_lands_under_capability_directory` | delta section 內容 | 路徑正確；validated=true |
| ART-EP-002 | 未知 type 錯誤逐字對齊 | EP | Medium (I3×L3) | 任一 change | Rust `unknown_type_reports_the_oracle_error` | `bogus` | stderr `Error: Unknown artifact type 'bogus'. Valid types: ...`；exit 1 |
| ART-EP-003 | spec 缺 capability 錯誤逐字對齊 | EP | Medium (I3×L3) | 任一 change | Rust `spec_without_capability_reports_the_oracle_error` | `new artifact spec` | stderr 用法錯誤逐字；exit 1 |
| ART-DT-001 | already-exists 先擋、`--force` 覆蓋但不跳過驗證 | DT | High (I4×L3) | proposal.md 已存在 | Rust `already_exists_errors_then_force_overwrites` | 兩次呼叫 | 第一次 exit 1（`Use --force to overwrite`）；加 --force 成功覆蓋 |
| ART-RB-001 | 驗證失敗不寫任何檔案 | RB | High (I5×L3) | change 無 proposal.md | Rust `proposal_validation_failure_does_not_create_file` | 無必要 section 的內容 | exit 1、逐字錯誤；檔案不存在 |
| ART-BVA-001 | not-found 錯誤無句尾句點（與 status 有句點不一致，oracle 特徵） | BVA | Medium (I3×L2) | 指定不存在的 change | Rust `nonexistent_explicit_change_has_no_trailing_period_in_error` | `--change nope` | stderr `Error: Change 'nope' not found`（無句點）；exit 1 |
| ART-VL-004 | human 輸出僅 stdin 模式附 `Content validated ✓` | VL | Low (I2×L3) | — | Rust `human_output_includes_validation_line_only_for_stdin` | 兩模式各一 | stdin 模式兩行、模板模式一行 |
| ART-SMK-001 | 三型驗證規則 + kebab/空 stdin 錯誤（probe 封閉） | EP | High (I4×L2) | oracle probe | Rust `artifact.rs` 單元測試（4 組：type 解析、kebab 驗證、per-type 驗證、檢查順序）+ 手動 oracle side-by-side（成功 JSON/模板/5 錯誤字串） | probe 矩陣 | 七層檢查順序與 design.md 一致 |

### capability: artifact-instructions（US-103，已實作）

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result |
|-------|-------------|-----------|------|-------------|-------|-----------|----------------|
| INST-VL-001 | 4 artifact 模式 JSON：key 順序、常數、DAG 欄位 | VL | High (I4×L3) | init + change | Rust `artifact_json_for_all_four_modes_has_canonical_order_constants_and_dag_fields`（`crates/spectra-cli/tests/instructions_integration.rs`） | 4 artifacts | keys 依序；instruction/template=常數；dependencies.done 反映檔案存在性 |
| INST-ST-001 | 無參數：全 done 才 apply，否則第一個未 done artifact | ST | High (I4×L3) | 不同完成度矩陣 | Rust `no_artifact_selects_first_incomplete_then_apply_when_all_are_done` | 完成度遞增 | 依 schema 順序選擇；全 done 進 apply |
| INST-ST-002 | apply ready：contextFiles 固定序、寬鬆 tasks、clean preflight | ST | High (I4×L3) | 四 artifact done | Rust `apply_ready_has_ordered_context_loose_tasks_progress_and_clean_preflight` | 任意單字元 checkbox、[P] 標記 | contextFiles 依 schema 序；[P] 剝除設 parallel；progress 正確 |
| INST-DT-001 | all_done / blocked 條件 key 消失矩陣 | DT | High (I4×L3) | tasks 全完成／零 checkbox／缺 tasks.md | Rust `apply_all_done_omits_conditional_keys` + `apply_blocked_distinguishes_a_missing_tasks_artifact_from_zero_checkboxes` | 三態 | all_done/blocked 無 preflight key；缺檔 blocked 帶 missingArtifacts=["tasks"]、零 checkbox blocked 不帶 |
| INST-ST-003 | preflight：missing/drift/staleness 優先序 | ST | High (I4×L3) | proposal 含 Affected code 區段 | Rust `preflight_reports_staleness_missing_files_and_drift_in_priority_order` | 缺檔+drift+stale 並存 | status=critical（missing 優先）；各清單正確 |
| INST-BVA-001 | 無 created 時 staleness 整鍵消失 | BVA | Medium (I3×L3) | `.openspec.yaml` 無 created | Rust `preflight_omits_staleness_when_created_is_missing` | 缺 created | 無 staleness key、不做 drift 比對 |
| INST-RB-001 | git last-commit 晚於 created → drifted（真 git repo 案例） | RB | High (I4×L2) | git repo、檔案 commit 日期可控 | Rust `preflight_reports_a_file_committed_after_the_change_date_as_drifted` | 後於 created 的 commit | driftedFiles 含 {path,lastCommit,changeCreated} |
| INST-EP-001 | 錯誤矩陣：未知 artifact／change 不存在／--skill 一律 Unknown | EP | Medium (I3×L3) | — | Rust `instructions_errors_match_the_oracle_contract` | 錯誤輸入矩陣 | 逐字錯誤；--skill 為 documented divergence（不移植專有 skill 文本） |
| INST-VL-002 | artifact/apply human 輸出逐位元 | VL | High (I4×L3) | 完整 change | Rust `human_artifact_and_apply_outputs_are_byte_exact` + 手動 oracle side-by-side（artifact×4 + no-arg + apply 三態） | 全模式 | 與 oracle 捕捉逐位元一致（Unlocks 區段含於內） |

### capability: change-analyze（US-104，已實作）

| TC-ID | Test Purpose | Technique | Risk | Precondition | Steps | Test Data | Expected Result |
|-------|-------------|-----------|------|-------------|-------|-----------|----------------|
| ANA-ST-001 | 無 artifact：4 維度 Skipped、snake_case、human pin、exit 0 | ST | High (I4×L3) | 空 change | Rust `empty_change_skips_all_dimensions_pins_snake_case_json_and_human_output`（`crates/spectra-cli/tests/analyze_integration.rs`） | 無 artifacts | 全 Skipped (insufficient artifacts)；findings=[]；artifacts_missing 全列；exit 0 |
| ANA-EP-001 | covMissingSpec 正反（Critical） | EP | High (I5×L3) | Capabilities token 有/無對應 spec | Rust `cov_missing_spec_positive_and_negative_contract` | backtick token 矩陣 | 正例 Critical + params/location 逐字；反例無 finding |
| ANA-EP-002 | covMissingTask 正反（Warning，case-insensitive substring） | EP | High (I4×L3) | tasks 含/不含 req 名 | Rust `cov_missing_task_positive_and_negative_contract` | req 名變體 | 契約逐字 |
| ANA-EP-003 | covDeltaValidation 正反（Critical，僅兩型錯誤觸發） | EP | High (I4×L3) | 重複 req／跨 section 同名 | Rust `cov_delta_validation_positive_and_negative_contract` | 兩型正例+免觸發反例 | 契約逐字；orphan scenario 等不觸發 |
| ANA-EP-004 | conDesignNotInTasks 正反（只認 ### 標題） | EP | Medium (I3×L3) | design ###/## 對照 | Rust `con_design_not_in_tasks_positive_and_negative_contract` | 標題層級矩陣 | ## 不觸發；keyword lowercase |
| ANA-EP-005 | ambNoScenario / ambAbstractScenario 正反 | EP | Medium (I3×L3) | scenario/example 有無 | Rust `amb_no_scenario_positive_and_negative_contract` + `amb_abstract_scenario_positive_and_negative_contract` | heading 邊界 | Warning／Suggestion 契約逐字 |
| ANA-BVA-001 | ambWeakLanguage：詞表優先序、substring 命中、每行一筆、行號 location | BVA | Medium (I3×L3) | 多弱語詞同行/跨行 | Rust `amb_weak_language_positive_and_negative_contract` | should/may/TBD/??? 矩陣（含 mayhem 誤中案例） | 依清單優先序回 canonical spelling；`<file>:<line>` |
| ANA-EP-006 | gapNoProposal / gapNoMainSpec / gapModifiedNotFound 正反 | EP | High (I4×L3) | 各 gap 條件 | Rust `gap_no_proposal_positive_and_negative_contract` + `gap_no_main_spec_positive_and_negative_contract` + `gap_modified_not_found_positive_and_negative_contract` | exact-name 邊界（substring 不算） | Critical/Warning 契約逐字；location=change directory 字面 |
| ANA-PW-001 | 維度 gating 矩陣（Coverage 2/3、Consistency=design+tasks、Ambiguity=specs、Gaps 任一） | PW | High (I4×L3) | artifact 有無組合 | Rust `analyze.rs` 單元測試（5 個）+ 整合案例交叉覆蓋 | artifact 組合對 | 每維度只在門檻達成時執行 |
| ANA-SMK-001 | 非平凡多 finding 案例與 oracle 逐位元 | ST | High (I5×L2) | macOS + oracle | 手動 oracle side-by-side：3-finding 案例 JSON+human 逐位元（主 session 驗收，含正向對照）；31 fixture probe 全紀錄見 design.md | 混合 findings | 除 documented determinism choices 外逐位元一致；exit 恆 0 |

---

## 2. Coverage Analysis

### Covered

| Scenario Slug | Corresponding TC IDs | Notes | 狀態 |
|---------------|----------------------|-------|------|
| status-empty-change | STAT-ST-001 | | ✓ |
| status-file-existence | STAT-ST-002 | 不 cascade | ✓ |
| status-complete | STAT-VL-001 | | ✓ |
| status-json-contract | STAT-VL-002, STAT-RB-001 | 含 oracle SBS | ✓ |
| new-change-no-scaffold | STAT-EP-001 | BREAKING | ✓ |
| scaffold-stdin | ART-VL-001 | | ✓ |
| scaffold-spec-capability | ART-VL-003, ART-EP-003 | | ✓ |
| scaffold-exists-force | ART-DT-001 | | ✓ |
| scaffold-validation | ART-RB-001, ART-SMK-001 | 三型規則見 design.md | ✓ |
| scaffold-unknown-type | ART-EP-002 | | ✓ |
| scaffold-template | ART-EP-001 | validated=false | ✓ |
| instructions-artifact-json | INST-VL-001 | | ✓ |
| instructions-apply | INST-ST-001, INST-ST-002, INST-DT-001, INST-ST-003 | 三態+preflight | ✓ |
| instructions-human | INST-VL-002 | 逐位元 | ✓ |
| analyze-insufficient | ANA-ST-001 | | ✓ |
| analyze-findings | ANA-EP-002 | covMissingTask 即 spec 例 | ✓ |
| analyze-finding-catalog | ANA-EP-001〜ANA-EP-006, ANA-BVA-001 | 10/10 正反 | ✓ |
| analyze-json-style | ANA-ST-001, ANA-SMK-001 | snake_case pin | ✓ |

### Partially Covered

（無——所有 spec scenario 皆有自動化測試；oracle side-by-side 為 macOS 手動步驟，屬既有 capture-golden 慣例，不列 partial）

### Completely Missing

（無）

### Redundant Items

| TC-ID | Duplicate of which TC | Recommended Action | 狀態 |
|-------|----------------------|--------------------|------|
| ANA-SMK-001 | 與 ANA-EP-* 概念重疊（皆驗 finding 契約） | 保留：ANA-EP-* 驗單一 finding 正反、ANA-SMK-001 驗多 finding 共存輸出與 oracle 逐位元傘測 | — 非真冗餘 |

---

## 3. 技法選用說明

- **EP**：finding 正反對、錯誤字串分類、模板/驗證模式分類——每個等價類一代表案例。
- **BVA**：not-found 句點有無（oracle 不一致特徵）、staleness created 缺失、弱語詞同行多命中。
- **DT**：already-exists × --force、apply 三態條件 key 出現矩陣。
- **ST**：DAG 狀態轉移（空→部分→全→刪除）、apply 模式選擇、preflight 優先序。
- **PW**：analyze 維度 gating 的 artifact 有無組合對。
- **RB**：golden pinning（模板漂移即紅）、驗證失敗不落檔、git drift 真 repo 案例——皆為 mob review 級風險點。
