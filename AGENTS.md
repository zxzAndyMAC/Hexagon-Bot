## Commands

- Dev: `npm run dev`（仓库根；拉起 Vite :1420 + Tauri 窗口）
- Test: `npm test`（cargo test --workspace + vitest）
- Check: `npm run check`（fmt + clippy -D warnings + oxlint + tsc）
- Rust 工具链经 rustup 安装，不在默认 PATH：命令前 `export PATH="$HOME/.cargo/bin:$PATH"`

## Layout

- `crates/hexagon-core/`：全部业务逻辑（唯一测试主接缝 = 本 crate 进程内 API）
- `src-tauri/`：Tauri 2 壳，只转发 IPC
- `ui/`：Vite + React + TS + Tailwind v4 + zustand

## Agent skills

### Issue tracker

Issues live as markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Canonical roles map 1:1 to `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
