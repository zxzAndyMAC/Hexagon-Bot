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

- **界面规范是唯一依据**：改 UI 控件、加组件、调样式、改交互、动布局或快捷键之前，先读 `docs/design/spec/README.md`，严格按规范执行（token 只取 `foundations.md`，组件按 `components.md` 的状态矩阵，行为按 `interaction.md`）。规范没写到的，先补规范（结构、变体、状态、出处）再写代码；不得就地造色、造圆角、造阴影
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

- **`pending_questions` 与 `agents.status` / `agents.removed_at` 的写路径只出现在属主模块**（`cards.rs` / `orchestra.rs`；`removed_at` 自 2026-10-17 review S3 起收进 `orchestra::write_agent_removed`，`team_removal` 只做守门判定）。检查：`rg "pending_questions|UPDATE agents SET (status|removed_at)" crates/hexagon-core/src` 的 SQL 命中只落属主；合法豁免=测试 helper 名（api/tests.rs）、schema 断言（db.rs）、泄漏扫描标签（credentials.rs `pending_questions:{id}`）、注释（D04/D09）
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

**测试/编译、截图、打包、验收或证据复用前，先读 [测试工作流](docs/agents/testing-workflow.md)。** 2026-10-23 负责人裁决：单票快速反馈；有界集成窗口共用全量/视觉；候选制品集中验收。逐票保留判据映射，不逐票重建、重签、重拍或请求主任务验收。执行批表的一行不自动触发一整套检查；以下专项覆盖仍须满足。

- **按影响选门禁**（2026-10-13 负责人裁决，替代“任何代码改动都全量”）：先检查 diff、调用者和受影响边界，再选以下最小充分检查；记录选择理由、源码快照、命令和结果。不要仅凭扩展名或改动行数决定测试范围。
  - **无行为变化**：纯文档、普通注释、空格和格式整理只做 diff 审查及适用的格式/生成物一致性检查，不跑 `npm run check` 或 `npm test`。先确认没有改变字符串、缩进语义、编译指令、可执行示例或配置；生成文件保持生成器原样输出。
  - **局部行为变化**：影响范围可证明局限于单模块、内部契约未变时，跑能复现问题的回归测试、受影响模块测试及对应 lint/typecheck/编译检查；通过即可交付，不因文件是 Rust 而自动升为全量。下面的 DTO、i18n、原生操控、裁决回路等专项门禁仍适用。
  - **全量触发**：跨模块行为变化、公共 API/IPC/持久化协议变化、依赖或构建链变化、权限/副作用边界变化、并发或生命周期变化，以及无法界定影响范围的修改，在覆盖该变化的有界集成窗口冻结点跑 `npm run check` + `npm test`；独立交付及明确风险停止点提前关闭窗口。全量成功必须覆盖最后一次相关行为变化，定向成功不能代替；窗口不得因换票无限延长，边界见测试工作流。
- **复用有效结果**：全量通过后只有已确认无行为变化的整理时，保留该次结果并补轻量检查；局部行为变化补受影响检查；再次触发全量条件才重跑全量。失败的门禁修复后重跑对应门禁，不能用定向绿覆盖全量红。重型构建/全量测试串行，恢复任务前先查进程，避免重复启动。
- **实施子任务自主验证**（2026-10-23 负责人裁决）：已授权分工内的测试、编译、lint/typecheck、生成绑定、诊断和修复复验自主执行，无需主任务逐次批准；重型检查按批次自动排队、取得资源后执行，指定验证者负责最终快照，主任务处理范围和语义冲突。产品内只读子代理另遵 ADR 0070/0102，不由本条获得写源码权限。批次与证据复用见 `.scratch/agent-execution-2026-10-23/README.md`。
- **改原生电脑操控或其 SDK 补丁**：先用 `npm run test:computer-use` 定向反馈；macOS 的 `npm test` 自动包含该门禁。构建/测试入口校验固定 SDK 与补丁前后哈希，直接 `swift test` 不替代干净依赖准备。原生无输入测试不代替实机验收。
- **无人值守网络重试**（2026-10-04 负责人裁决）：网络中断前两次各等 1 分钟；连续第三次失败后等 5 分钟再试，成功清零。配置/权限/代码错误先诊断，不按网络故障循环。长任务在当前执行批的唯一运行记录维护快照、进程、日志及下一步；`.scratch/fullstack-qa-2026-10-03/handoff.md` 仅服务原 QA 任务，其他入口只链接当前记录。恢复后核实进程及结果，不能凭旧记录重启仍在运行的任务。
- **改 `#[derive(ts_rs::TS)]` 结构**：重跑 `cargo test export_bindings`（测试名过滤器不是文件名），`ui/src/gen/` diff 随改动同次入提交（D06）
- **截图回归**（ui-redesign 票 17，单列门禁，不在 `npm test` 内）：改 `ui/src/components/`、`ui/src/index.css`、token、i18n 文案（任一语言）、`ui/src/api.ts` 的 dev mock 数据，或升级 UI 依赖/字体/Playwright 时，实施中按风险跑受影响用例，最终集成窗口统一跑 `npm run test:visual`，多票引用同一次结果；后续交付无相关变化则复用（Playwright + 无头 Chromium，Vite dev :1460 + `?mock*` 数据，1440×900、减少动效、固定时钟/时区/语言；深浅两套 + fr 主界面，约 67 张）。出 diff：是本次有意改动 → 用 `npm run test:visual:update` 的定向选择入口仅更新对应基线，逐张看 `ui/visual/__screenshots__/` 的实际变更 diff，提交说明写清哪几张为何变；不是有意改动 → 修代码，不准为过门重拍。失败产物在 `ui/test-results/`（`*-diff.png`）。基线绑定 macOS + 锁定的 Playwright Chromium 版本，换机/升 Playwright 后先在干净 main 上重拍一次单独提交。阈值 `maxDiffPixelRatio 0.001`（≈1300 像素，只容抗锯齿噪声）。更新命令不代替最终普通比较；不为每票制作重复截图报告
- **改 `ui/src/i18n/locales/`**：`node ui/scripts/check-i18n.mjs`（七语言 key 集合对账，已挂 `npm run check`）+ 一种非英文语言手测冒烟
- **修 bug**：同一次改动附回归测试——修复即文档，commit 会被遗忘，测试跟着代码走；新判定器必须配 proptest（D12）
- **新 UI 界面/控件**：同步 vitest
- **裁决回路改动**（`decisions.ts`/`PendingCards`/`stageops`/`TopBar`/`store.ts` 的 pending|streams|modalScope 面）：过黄金标准手测——离开 20 分钟归来，30 秒内看清「发生了什么、哪些在等拍板、最严重的是什么」并完成第一笔裁决。该路径在最终相关版本按工作台用户旅程集中跑，多票各映射判据，不逐票重演；实现过程中的裁决风险及时验证。5 分钟跑法：造一张待决卡 → ①设置页打开按 ⌘↵ 应被拦+提示；②回工作台 ⌘↵ 裁决成功；③顶卡为 publish 时 ⌘↵ 只出提示不出裁决
- **E2E 纪律**：E2E 只留黄金标准路径（启动→向导→首指令→首卡→裁决→推进→归来摘要）；下层已测的不重复上浮；无 e2e 设施期间用半自动跑法（`.scratch/qa-alloy/cases.md` TC-E-0001）
- **行为变更触及既有测试断言**：改测试与改代码同次提交，并在测试 diff 处注释说明行为为何变；静默删测试视为未过门

## 多 Agent 并行实现（编排）

2026-10-20 负责人流程审计沉淀（owner-followups-1015：每票实现者 15–25 万 token、35–55 分钟，大头花在重复门禁与重读上下文）。编排方（主会话）派实现者 subagent 跑一批票时适用。

- **门禁分层**：实现者只跑定向检查（回归/受影响测试 + 对应 lint/typecheck/tokens/i18n）；`npm run check`、`npm test`、全量 `npm run test:visual` 由编排方每批在整合分支顶端跑一次（同「测试守门」「截图回归·跑的时机」）。被否决：每票各跑全量——重复约 3000 个测试，还在重型锁上串行排队
- **简报只有一份且不自相矛盾**：规则变了就整份重写实现者简报，不在末尾追加「补丁段」；已在跑的实现者用一条消息通知改动。出处：追加段与原文冲突，实现者照旧段直接改了共享文档
- **模型按票选，派活必须显式传 model**：默认 subagent 模型在本环境 404；核心/IPC/协议类票用 opus；`sonnet` 别名此环境也 404（2026-10-20），可用前不要派
- **小票打包**：同区域的小修（单个 CSS/文案/状态修正）2–3 张合给一个实现者、一条分支，省去每人重读 AGENTS.md / spec / 设计规范与准备独立工作目录
- **额度中断后新开，不续接**：限额重置后不要 SendMessage 续接长对话的实现者（缓存已过期，整段历史按原价重发）；新开实现者，从其工作目录与进度记录 `notes/t<NN>.md` 接着做。实现者每到一个节点就覆盖更新进度记录：分支、提交、已完成、剩余、下一步、在跑进程
- **新截图只按票要求加**：截图库越大，每次全量越慢
- **隔离工作目录写不了软链出去的 gitignored 文档**（`docs/` 等）：实现者把文档改动写成幂等脚本交回，编排方审过后执行；不得绕过
- **合并检查按改动面**：分支动了 Rust 才跑 `cargo check --workspace`，纯前端只跑 `tsc -b`

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

实施、接续或验收 Hooks、Runtime hardening、Computer Use SDK、运行记录、子代理受控验证票据时，先读[总执行指南](.scratch/agent-execution-2026-10-23/README.md)，再按当前票加载规格与验证要求。

### Triage labels

Canonical roles map 1:1 to `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
