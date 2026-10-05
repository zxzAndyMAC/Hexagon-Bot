## Commands

- Dev: `npm run dev`（仓库根；拉起 Vite :1420 + Tauri 窗口）
- Test: `npm test`（cargo test --workspace + vitest）
- Check: `npm run check`（fmt + clippy -D warnings + check-arch.sh + oxlint + tsc + check-tokens + check-i18n）
- Rust 工具链经 rustup 安装，不在默认 PATH：命令前 `export PATH="$HOME/.cargo/bin:$PATH"`

## Layout

- `crates/hexagon-core/`：全部业务逻辑（唯一测试主接缝 = 本 crate 进程内 API）
- `src-tauri/`：Tauri 2 壳，只转发 IPC
- `ui/`：Vite + React + TS + Tailwind v4 + zustand

## UI 约定（ADR 0051）

- 新功能注册的 action 必须进 keymap（`mod+X` 规范形，双平台渲染 ⌘/Ctrl）；按钮 tooltip 必须显示当前绑定
- 界面文案全走 i18next key（七语言）；Agent 产物/项目内容不翻译
- **术语词典**：`docs/glossary.html` 是领域+界面术语的唯一详本（定名/代码锚点/i18n key；本地文档不进 git，同 ADR）。任何改动 UI 控件、领域概念、事件 kind、Tauri 命令、i18n key、CSS 类的工作必须在同一次改动中更新该文件；讨论与 bug 报告以其「定名」为准

## 注释纪律（出处约定）

源自 OpenWorker 的注释实践：反直觉规则必须挂「当初为什么」——事故日期/票号/owner 裁决 + 被否决的替代方案。效果：半年后读代码的人（或 agent）不只看到规则，还看到当初为什么错、什么不能改。

- **权限/提示词/协议类代码**：非直觉分支必须带出处（票号/incident/owner 裁决/spec 条目），改的人先读注释再动手
- **fail-closed 不对称性显式声明**：分类器/判定器注释里写清代价模型——"false negative 花一次人工，false positive 花一次未审副作用"——并说明实现偏向哪边
- **修复即文档**：修 bug 时在代码里留"这个写法曾经怎么坏的"，不只留在 commit message 里（commit 会被遗忘，注释跟着代码走）

## 架构接缝规则

arch-review 2026-09 治理沉淀（依据 `.scratch/arch-review/report.md` 诊断卡）。每条附可执行的检查面；检查只剩人工评审的规则不进本节。

- **`pending_questions` 与 `agents.status` 的写路径只出现在属主模块**（`cards.rs` / `orchestra.rs`）。检查：`rg "pending_questions|UPDATE agents SET status" crates/hexagon-core/src` 的 SQL 命中只落属主；合法豁免=测试 helper 名（api/tests.rs）、schema 断言（db.rs）、泄漏扫描标签（credentials.rs `pending_questions:{id}`）、注释（D04/D09）
- **core 内部模块不得 `use crate::api`**——门面只被壳层调用。白名单：`scenario.rs`、`setup.rs`、`replay.rs`（回放/向导是门面上方的驱动方，用真门面是设计）；`#[cfg(test)]` 内 `Workbench::for_test` 是合法测试接缝不算依赖；`errcode.rs` 是错误码注册表，按设计必须点名全部错误类型。检查：`scripts/check-arch.sh`——`use crate::api` 只落白名单；`crate::api::` 路径引用除 `Workbench`（`for_test` 测试接缝，`cfg(test)` 编译闸天然兜底）外只落白名单 + `errcode.rs` + `api.rs`（D08，票 09 清零 judge 残留）
- **IPC 返回值必须是 serde 结构体**：禁止 `Vec<Value>`/`json!` 逐行拼装响应，payload 字段不得嵌字符串化 JSON。检查：`rg 'Result<Vec<Value>|Result<Value' crates/hexagon-core/src/api.rs` 白名单收敛（D06）
- **跨 IPC 的 DTO 必须 `#[derive(ts_rs::TS)]` + `#[ts(export, export_to = ...)]`**，`export_to` 相对 crate 根：hexagon-core 用 `../../../ui/src/gen/`、src-tauri 用 `../../ui/src/gen/`。重生：`cargo test export_bindings`；64 位整数字段必须钉 `#[ts(type = "number")]`（ts-rs v11 默认 i64/u64→bigint，与 JSON number 不符）。`ui/src/gen/` 入 git，UI 不得手写同名 DTO（D06/ADR 0054）
- **Tauri command 分 read/control/turn 三组注册，read 组禁止触碰 `state.wb`**。检查：`rg "state.wb" src-tauri/src/lib.rs` 每处命中的当行或紧邻下一行须挂标签——`D01-ok`（mutation 路径，如 `refresh_providers`）或 `D01-exempt`（read 侧探针不调 wb 方法，如 `project_open` 的 `is_some`）。`generate_handler!` 是扁平单表，三组只是约定，行内标签是唯一可机检边界（`scripts/check-arch.sh`）
- **模型槽回退链只在 `provider_config::resolve_slot` 一处实现**（原 providers.rs，票 10 更名解碰 provider/providers）。检查：`rg 'get("default")' crates/hexagon-core/src` 命中只许 `provider_config.rs`（resolve_slot 本体）或挂 `D13-exempt` 行内理由（`scripts/check-arch.sh`）
- **`System` 事件的 `payload.kind` 子类词表集中登记于 `trace.rs`**，新增子类须同改词表与 `docs/glossary.html`。检查：登记完整性由 `trace.rs` 测试 `system_subkinds_stay_registered` 守（写入后读回校验）；`check-arch.sh` 提示级列出裸字面量写入点，常量化是未做的重构面（D10）
- **新判定器（judge/invariant/权限类判定面）必须配 proptest 属性测试**，不只样例测试；回归种子入 `proptest-regressions/`。检查：评审清单——新判定面 PR 必须含 `proptest!` 块钉不变量（D12）

- 以上检查面统一固化于 `scripts/check-arch.sh`（QA 委托 GR-06），随 `npm run check` 执行；新命中无白名单/行内豁免即红

## 测试守门

2026-09 从 0 到 1 全面 QA 委托沉淀（委托提示词与用例库：`.scratch/qa-alloy/`）。任何代码改动交付前按改动类型过门；说不清自己触发了哪条 = 没过门。

- **任何代码改动交付前**：`npm run check` + `npm test` 全绿。2026-10-03 负责人裁决：修复过程中先跑能复现该问题的定向回归及受影响检查，同一批小改动结束再统一全量；跨模块、大改动、权限/协议边界改变提前全量。记录每批源码快照、命令、结果，最后一次源码变化后的全量成功才算交付。豁免仅纯文档（`.md`），须在交付说明留痕
- **改 Rust**：调试阶段允许单模块/单测反馈；批次交付必须跑 `npm test` 全量，定向成功不能替代最终全量
- **无人值守网络重试**（2026-10-04 负责人裁决）：网络中断前两次各等 1 分钟；连续第三次失败后等 5 分钟再试，成功清零。配置/权限/代码错误先诊断，不按网络故障循环。长任务持续更新 `.scratch/fullstack-qa-2026-10-03/handoff.md`；恢复后核实进程及结果，不能凭旧记录重启仍在运行的任务。
- **改 `#[derive(ts_rs::TS)]` 结构**：重跑 `cargo test export_bindings`（测试名过滤器不是文件名），`ui/src/gen/` diff 随改动同次入提交（D06）
- **改 `ui/src/i18n/locales/`**：`node ui/scripts/check-i18n.mjs`（七语言 key 集合对账，已挂 `npm run check`）+ 一种非英文语言手测冒烟
- **修 bug**：同一次改动附回归测试——修复即文档，commit 会被遗忘，测试跟着代码走；新判定器必须配 proptest（D12）
- **新 UI 界面/控件**：同步 vitest
- **裁决回路改动**（`decisions.ts`/`PendingCards`/`stageops`/`TopBar`/`store.ts` 的 pending|streams|modalScope 面）：过黄金标准手测——离开 20 分钟归来，30 秒内看清「发生了什么、哪些在等拍板、最严重的是什么」并完成第一笔裁决。5 分钟跑法：造一张待决卡 → ①设置页打开按 ⌘↵ 应被拦+提示；②回工作台 ⌘↵ 裁决成功；③顶卡为 publish 时 ⌘↵ 只出提示不出裁决
- **E2E 纪律**：E2E 只留黄金标准路径（启动→向导→首指令→首卡→裁决→推进→归来摘要）；下层已测的不重复上浮；无 e2e 设施期间用半自动跑法（`.scratch/qa-alloy/cases.md` TC-E-0001）
- **行为变更触及既有测试断言**：改测试与改代码同次提交，并在测试 diff 处注释说明行为为何变；静默删测试视为未过门

## 诊断记录

2026-09-23 负责人裁决。轨迹是业务事实，测试断言打轨迹和门面结果。诊断记录只用来在手动测试时对上「走了哪一支」。

- 新增判定分支（权限、自治、执行判定、点名路由、槽位回退、门面拒绝）时写一条诊断记录：分类、项目/Agent/激活/轨迹 id、分支、原因码、耗时
- 分类只有四档。判定、槽位的正常回退、宿主记 Debug。拒绝，以及槽位上的失败，记 Warn
- 记录里放种类、id、分支、原因码、耗时。提示词、钥匙、工具输出、消息正文留在轨迹和消息里
- 同一份进本机日志目录，并供设置「日志」页读取。不送到外部观测服务
- 测试不断言日志字符串

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical roles map 1:1 to `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
