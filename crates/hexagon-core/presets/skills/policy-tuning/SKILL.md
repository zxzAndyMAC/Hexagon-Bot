---
name: policy-tuning
description: 策略研发流程：读证据池 → 只改旋钮副本 → 回放验证 → 携证据提交提案
---

# 策略调优（policy-dev 工作流）

你能动的只有「策略旋钮」：knobs.judge / knobs.flag_patience / knobs.auto_backfill /
knobs.consult_auto_wake，以及阶段级 stamp_point / backfill_edges / consult_wake。
阶段顺序、角色名单、产物、验收、检验是「流程定义」——不在你的权限内，
提案只要碰它们就会被机械校验拒掉。

## 流程

1. 读证据：回放报告、打回理由码分布（repairable/ambiguous/hard-blocked/budget-exceeded）、
   决策点分歧。先形成「哪个旋钮可能改善哪个信号」的假设。
2. 在流程包副本上改旋钮（不改钉住包、不改运行实例）。
3. 用固定场景跑回放：同一脚本世界分别跑基线包和候选包。
4. 打包提案：动机写假设、改动面写旋钮 diff、预期收益写回放信号、
   验证方法挂 ```replay 证据块。提交走普通提案队列。

## 边界（不可协商）

- 你的产出永远是「提案」，不是「生效」——负责人盖章前一切不生效。
- judge 的建议（含你自己的判断）都不授权部署；盖章权在负责人。
- 回放只证明「这个场景里更好」，不外推「所有场景都好」——提案里不许写绝对化结论。
- 一次回放一个变量：同提案混多个旋钮改动会让归因失真，拆成多份提案。
