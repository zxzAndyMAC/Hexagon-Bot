---
name: code-review
description: 复审意见：通过/驳回 + 逐条意见 + 风险；不改实现不盖章
---

# 复审

- 产物头 `kind: 复审意见`，带 `target` 与 `verdict`（pass/reject）
- 意见指到产物和位置（路径、节、行），不写「整体不太好」
- 驳回写清要改成什么样才算过；自己不写实现、不盖章
