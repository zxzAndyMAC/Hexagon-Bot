// 截图回归共用驱动（票 ui-redesign 17）。只走 dev mock（api.ts 的 ?mock* 旗标），不碰真宿主。
import { expect, type Page } from '@playwright/test'

export type Theme = 'light' | 'dark'
export type Lang = 'zh-CN' | 'fr'

// 固定时钟：相对时间（「3 分钟前」）、倒计时、用量区间都以此为「现在」。
// 选 2026-10-12（负责人确认截图回归那天）上午，避开跨日/跨月边界。
export const FIXED_NOW = new Date('2026-10-12T10:00:00+08:00')

type BootOpts = { query?: string; theme?: Theme; lang?: Lang }

export async function boot(page: Page, { query = '', theme = 'light', lang = 'zh-CN' }: BootOpts = {}) {
  await page.clock.setFixedTime(FIXED_NOW)
  // 标本页（?specimen）的主题默认「跟随系统」，不读 hexagon.theme——只写持久化键时
  // feedback-*-dark 基线实为浅色、与 light 逐字节相同（2026-10-17 合并时发现）。系统配色一并模拟。
  await page.emulateMedia({ colorScheme: theme })
  // 主题/语言走持久化键再载入，与真实启动同一条路径（页内切换可能拿到另一份 store 模块实例）
  await page.addInitScript(([th, lg]) => {
    localStorage.setItem('hexagon.theme', th)
    localStorage.setItem('hexagon.lang', lg)
  }, [theme, lang] as const)
  await page.goto(query ? `/?${query}` : '/')
  await settle(page)
}

/** 等字体、网络、两帧布局；mock 数据有少量 setTimeout 投递，留 400ms 尾巴。 */
export async function settle(page: Page, ms = 400) {
  await page.waitForLoadState('networkidle')
  await page.evaluate(async () => {
    await document.fonts.ready
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
  })
  await page.waitForTimeout(ms)
}

/**
 * 在页面已加载的那份 store 模块上执行 body（`s` = useUiStore）。
 * Vite 给热更新过的模块挂 ?t= 后缀；直接 import('/src/store.ts') 可能拿到第二份实例，状态改了画面不动（票 16 实测）。
 */
export async function store<T = unknown>(page: Page, body: string): Promise<T> {
  return page.evaluate(async (src) => {
    const url = performance.getEntriesByType('resource').map((e) => e.name).find((n) => n.includes('/src/store.ts')) || '/src/store.ts'
    const { useUiStore } = await import(/* @vite-ignore */ url)
    return new Function('s', `return (async () => { ${src} })()`)(useUiStore)
  }, body) as Promise<T>
}

/** 当前语言下的界面文案（页面已加载的那份 i18n 实例），让定位器不绑死某一种语言。 */
export async function tr(page: Page, key: string): Promise<string> {
  return page.evaluate(async (k) => {
    const url = performance.getEntriesByType('resource').map((e) => e.name).find((n) => n.includes('/src/i18n/index.ts')) || '/src/i18n/index.ts'
    const { default: i18n } = await import(/* @vite-ignore */ url)
    return i18n.t(k) as string
  }, key)
}

export async function snap(page: Page, name: string) {
  await settle(page, 250)
  await expect(page).toHaveScreenshot(`${name}.png`)
}

export const THEMES: Theme[] = ['light', 'dark']
