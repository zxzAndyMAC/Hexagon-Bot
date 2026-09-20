## Commands

- Dev: `npm run dev`（仓库根；拉起 Vite :1420 + Tauri 窗口）
- Test: `npm test`（cargo test --workspace + vitest）
- Check: `npm run check`（fmt + clippy -D warnings + oxlint + tsc）
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
- **core 内部模块不得 `use crate::api`**——门面只被壳层调用。白名单：`scenario.rs`、`setup.rs`、`replay.rs`（回放/向导是门面上方的驱动方，用真门面是设计）；`#[cfg(test)]` 内 `Workbench::for_test` 是合法测试接缝不算依赖；`errcode.rs` 是错误码注册表，按设计必须点名全部错误类型。检查：`rg "use crate::api|crate::api::" crates/hexagon-core/src` 命中只落白名单+api.rs 自身（D08，票 09 清零 judge 残留）
- **IPC 返回值必须是 serde 结构体**：禁止 `Vec<Value>`/`json!` 逐行拼装响应，payload 字段不得嵌字符串化 JSON。检查：`rg 'Result<Vec<Value>|Result<Value' crates/hexagon-core/src/api.rs` 白名单收敛（D06）
- **跨 IPC 的 DTO 必须 `#[derive(ts_rs::TS)]` + `#[ts(export, export_to = ...)]`**，`export_to` 相对 crate 根：hexagon-core 用 `../../../ui/src/gen/`、src-tauri 用 `../../ui/src/gen/`。重生：`cargo test export_bindings`；64 位整数字段必须钉 `#[ts(type = "number")]`（ts-rs v11 默认 i64/u64→bigint，与 JSON number 不符）。`ui/src/gen/` 入 git，UI 不得手写同名 DTO（D06/ADR 0054）
- **Tauri command 分 read/control/turn 三组注册，read 组禁止触碰 `state.wb`**。检查：`rg "state.wb" src-tauri/src/lib.rs` 命中只落 turn/mutation 组（D01）
- **模型槽回退链只在 `provider_config::resolve_slot` 一处实现**（原 providers.rs，票 10 更名解碰 provider/providers）。检查：`rg 'or_else.*"default"' crates/hexagon-core/src` 归零（D13）
- **`System` 事件的 `payload.kind` 子类词表集中登记于 `trace.rs`**，新增子类须同改词表与 `docs/glossary.html`。检查：`rg 'json!\(\{"kind":' crates/hexagon-core/src` 字面量只来自词表常量（D10）
- **新判定器（judge/invariant/权限类判定面）必须配 proptest 属性测试**，不只样例测试；回归种子入 `proptest-regressions/`。检查：评审清单——新判定面 PR 必须含 `proptest!` 块钉不变量（D12）

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical roles map 1:1 to `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
