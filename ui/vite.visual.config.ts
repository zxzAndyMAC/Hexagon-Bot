// 截图回归专用 dev server（票 ui-redesign 17）。只被 playwright.visual.config.ts 的 webServer 拉起，
// 不改仓库根的 dev 命令（Tauri 固定 1420 + strictPort）。
// fs.allow：worktree 里 ui/node_modules 常是指向主仓的符号链接，真实路径在 Vite 默认根之外，
// @fontsource 的 woff2 会被 403 拦下，字体悄悄回退成系统字体 → 每台机器基线都不同（票 16B 实测）。
// 把 node_modules 的真实路径加进白名单，Inter / JetBrains Mono 就始终从本地包加载。
import { realpathSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { mergeConfig, searchForWorkspaceRoot } from 'vite'
import base from './vite.config.ts'

const root = fileURLToPath(new URL('.', import.meta.url))

export default mergeConfig(base, {
  server: {
    port: 1460,
    strictPort: true,
    fs: { allow: [searchForWorkspaceRoot(root), realpathSync(`${root}node_modules`)] },
  },
})
