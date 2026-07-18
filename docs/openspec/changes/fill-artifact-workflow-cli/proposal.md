# Proposal：fill-artifact-workflow-cli

> 版本：v1.0 | 日期：2026-07-18 | 狀態：Draft
> 來源：sdd plugin `/spectra-propose` 流程在 openspectra 0.2.1 上踩到的指令缺口（使用者截圖回報）

## Why

yibi-stack sdd plugin 的 spec 工作流（`/spectra-propose` 等）假設的 4 個指令在
openspectra 0.2.1 都不存在：`instructions <artifact> --json`、`new artifact
<type> --stdin`、`status --json`、`analyze --json`。這些指令在閉源 oracle
（Spectra.app 2.3.1，本機 `~/.local/bin/spectra`）全部存在且已完成介面探測。
補齊後 openspectra 才能作為 sdd plugin 的 drop-in 替代品跑完整 artifact 工作流。

## What Changes

1. **workflow schema 基礎模組**（`spectra-core::schema`）：內建 `spec-driven`
   schema——artifact DAG（`proposal → design`；`proposal → specs → tasks`，
   design 不是 tasks 的依賴——實測確認）、每個 artifact 的
   `outputPath`／description／instruction／template 文字（自 oracle 逐字擷取）、
   狀態推導（`done`/`ready`/`blocked`）。
2. **`spectra status [--change] [--json]`**：顯示 artifact DAG 狀態。
3. **`spectra new artifact <TYPE> [CAPABILITY] [--change] [--stdin] [--force] [--json]`**：
   建立單一 artifact 檔，含 per-type 內容驗證（如 proposal 必須有
   `## Why`/`## Problem`/`## Summary` 之一）。
4. **`spectra instructions [ARTIFACT] [--change] [--json]`**：輸出 artifact 的
   AI 指令＋模板；無參數／`apply` 形態輸出 apply 模式（context files、tasks
   進度、preflight）。
5. **`spectra analyze [CHANGE] [--json]`**：4 維度（Coverage/Consistency/
   Ambiguity/Gaps）× 10 種 finding 的一致性分析引擎。
6. **BREAKING：`new change` 不再 scaffold artifact 檔**：oracle 的 `new change`
   只寫 `.openspec.yaml`（不建 proposal.md/design.md/tasks.md）；0.2.1 的
   scaffold 行為會讓 `status` 一開始就全 `done`、`new artifact` 永遠撞
   「already exists」，整條 DAG 工作流失效，必須對齊 oracle。

## Non-Goals

- **`--agent claude` 全域旗標**：oracle 2.3.1 亦無此旗標（實測
  `error: unexpected argument '--agent'`）——skill 是為更新版 CLI 寫的，
  此項應修 skill 端，CLI 端無 oracle 可對齊，排除。
- oracle 其餘未移植指令（`search`/`schema`/`config`/`feedback`/`demo`/
  `completion`/`in-progress`/`update`/`templates`/`schemas`）：sdd plugin 未依賴，
  不在本次範圍。
- 多 schema 支援：只內建 `spec-driven`（oracle `schemas` 也只列這一個）；
  `--schema` 旗標收下但僅接受 `spec-driven`。
- i18n locale 切換：`locale` 欄位輸出固定 `English`（oracle 預設）；
  `zh-TW` 模板留待後續。

## Step 1a — 四元素萃取

| 元素 | 內容 |
|------|------|
| **Actors** | sdd plugin（agent 驅動的 spec 工作流）、CLI 使用者、CI 腳本 |
| **Actions** | 查 DAG 狀態、以 stdin 建 artifact、取 artifact 指令／模板、分析 artifact 一致性 |
| **Data** | `openspec/changes/<name>/{proposal.md,design.md,tasks.md,specs/**/*.md}`、`.openspec.yaml`（schema 欄位）、4 種 `--json` 合約（見 design.md §Oracle 合約） |
| **Constraints** | 所有輸出 shape／錯誤字串／exit code 以 oracle 2.3.1 實測為準；`analyze` 恆 exit 0；`new artifact` 驗證失敗 exit 1；`status --json` camelCase、`analyze --json` snake_case（oracle 即如此，不得「修正」） |

## User Stories（總覽，AC 詳見各 capability spec）

- **US-101（capability: `workflow-status`）**：agent 在 change 工作流中隨時以
  `spectra status --json` 取得下一步可做的 artifact。
- **US-102（capability: `artifact-scaffold`）**：agent 產出 artifact 內容後以
  `spectra new artifact <type> --stdin` 寫入正確路徑並獲得即時驗證。
- **US-103（capability: `artifact-instructions`）**：agent 以
  `spectra instructions <artifact> --json` 取得該 artifact 的撰寫指令與模板。
- **US-104（capability: `change-analyze`）**：agent 完成 artifacts 後以
  `spectra analyze --json` 找出跨 artifact 的缺口與不一致。

## Impact

- `crates/spectra-core`：新增 `schema.rs`（DAG＋模板）、`artifact.rs`（scaffold＋
  驗證）、`analyze.rs`（findings engine）、`instructions.rs`；`change.rs` 的
  `create` 移除 artifact scaffold（**BREAKING**，影響既有 `new change` 測試）。
- `crates/spectra-cli`：`main.rs` 新增 4 個子指令。
- `docs/reverse-engineering/`：新增 `artifact-workflow.md`（status/new artifact/
  instructions）與 `analyze.md`，同 PR 內完成（repo 規範）。
- sdd plugin 端後續需把 `--agent` 假設移除（本 repo 之外，另開 issue）。
