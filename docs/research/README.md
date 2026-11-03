# OpenSpectra 研究與實證評測紀錄（Research & Evaluations）

本目錄存放 OpenSpectra 在架構演進、Agent Skills 設計與驗證機制上的實證研究與控制實驗報告。

## 報告索引

- [**OpenSpectra Agent Skills 實證評測研究報告（2026-11）**](skill-evaluation-2026-11.md)
  - 涵蓋 2026 年 Agent Skills 規模退化（Skill Shadowing / Implicit Routing Collapse）文獻查證，以及針對 `propose`、`apply`、`verify` 三大內嵌工作流的 **4 階段、148 次隔離環境配對與 $2 \times 2$ 因子試驗**。
  - 核心結論：**維持現行內嵌 Skills 原文不變**；證實「環境寫入限制與確定性檢查工具（Runner Guard / Mechanical Checkers）」能 100% 消除越權與格式缺陷，而過度堆疊防禦性 Prompt 反而會增加 18.6% Token 消耗並引發長尾超時（Overthinking Timeout）。
  - 關聯架構決策：[ADR-0001：OpenSpectra 規格深化、測試追蹤與驗證證據升級計畫](../adr/0001-openspectra-quality-workflow-upgrade.md)
