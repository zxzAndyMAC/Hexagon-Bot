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

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical roles map 1:1 to `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
