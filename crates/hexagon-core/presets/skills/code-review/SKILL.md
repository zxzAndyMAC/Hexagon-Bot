---
name: code-review
description: 复审意见：通过/驳回 + 逐条意见 + 风险；不改实现不盖章
---

# 复审

1. 用 `artifact_read` 读取目标完整正文，保存返回的 `review_target`。若它为 null，先补齐目标文件或重读稳定的完整产物。
2. 完成复审后，用 `artifact_write` 提交意见。头部包含 `kind: 复审意见`、真实实例 `author`、目标路径或 id `target`、`verdict: pass/reject`，以及 `target_evidence: <原 review_target 的单行 JSON>`。
3. 提交若报告目标变化，重新读取并复审；保留原证据直至本次提交完成，不能把新版本摘要拼到旧意见上。

- 意见指到产物和位置（路径、节、行），不写「整体不太好」
- 驳回写清要改成什么样才算过；自己不写实现、不盖章
