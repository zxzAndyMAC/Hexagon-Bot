// 截图回归（票 ui-redesign 17）。一条命令：仓库根 `npm run test:visual`；更新基线：`npm run test:visual:update`。
// 不进 `npm test`：基线绑定 macOS + 固定 Chromium 版本的渲染（字体栅格化随系统变），触发条件见 AGENTS.md「测试守门」。
import { defineConfig } from '@playwright/test'

const PORT = 1460

export default defineConfig({
  testDir: './visual',
  // *.visual.ts 而非 *.spec.ts：vitest 默认收 *.spec.*，避免截图用例混进 `npm test`
  testMatch: '**/*.visual.ts',
  snapshotPathTemplate: '{testDir}/__screenshots__/{arg}{ext}',
  // 串行：同一 dev server、同一台机器，避免并发抢 CPU 导致首帧布局/字体时序抖动
  workers: 1,
  fullyParallel: false,
  retries: 0,
  reporter: [['list']],
  timeout: 60_000,
  expect: {
    toHaveScreenshot: {
      // 0.001 ≈ 1440×900 上 1300 像素：容得下亚像素抗锯齿噪声，
      // 容不下一个按钮换色、一行文字位移或一个图标消失（这些都在数千像素以上）。
      maxDiffPixelRatio: 0.001,
      animations: 'disabled',
      caret: 'hide',
      scale: 'css',
    },
  },
  use: {
    baseURL: `http://localhost:${PORT}/`,
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 1,
    reducedMotion: 'reduce',
    timezoneId: 'Asia/Shanghai',
    locale: 'zh-CN',
  },
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
  webServer: {
    command: 'npx vite --config vite.visual.config.ts',
    url: `http://localhost:${PORT}/`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
})
