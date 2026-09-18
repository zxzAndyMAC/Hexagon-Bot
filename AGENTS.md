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

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical roles map 1:1 to `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
