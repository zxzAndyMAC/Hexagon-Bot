// 截图回归 · 工作台面（票 ui-redesign 17）：外壳、待决坞、画布、托管、用量、恢复。深浅两套 + fr 主界面。
import { test, type Page } from '@playwright/test'
import { boot, settle, snap, store, THEMES, type Lang, type Theme } from './helpers.ts'

/** 时间线跳到某条事件并藏起钉住条（与票 16 截图同一取景）。 */
async function jump(page: Page, seq: number) {
  await page.addStyleTag({ content: '.tl-pin { display: none !important }' })
  // 反复跳到落点不再变：第一次跳转时虚拟列表还在量行高，落点随量测进度漂
  // （同一基线前后差 2% 像素；固定跳两次仍偶发漂，票 17 实测）。连续两次 scrollTop 相同才拍。
  const top = () => page.evaluate(() => [...document.querySelectorAll('[data-virtuoso-scroller]')].map((e) => e.scrollTop).join(','))
  let prev: string | null = null
  for (let i = 0; i < 6; i++) {
    await page.evaluate((d) => window.dispatchEvent(new CustomEvent('hexagon:timeline-jump', { detail: d })), seq)
    await settle(page, 600)
    const now = await top()
    if (now === prev) break
    prev = now
  }
}

async function shell(page: Page, theme: Theme, lang: Lang, tag: string) {
  await boot(page, { theme, lang })
  await snap(page, `shell-dock-open-${tag}`)
  await store(page, 's.getState().closePendingDialog()')
  await snap(page, `shell-dock-collapsed-${tag}`)
}

async function canvas(page: Page, theme: Theme, lang: Lang, tag: string) {
  await boot(page, { theme, lang })
  await store(page, 's.getState().closePendingDialog(); s.getState().openCanvas("artifacts")')
  await snap(page, `canvas-side-${tag}`)
  await store(page, 's.getState().toggleCanvasFull()')
  await snap(page, `canvas-full-${tag}`)
}

async function managed(page: Page, theme: Theme, lang: Lang, tag: string) {
  await boot(page, { theme, lang, query: 'mockmanaged' })
  await store(page, 's.getState().closePendingDialog()')
  await jump(page, 122)
  await snap(page, `managed-strip-${tag}`)
  await page.locator('[data-managed-state]').first().click()
  await page.locator('[data-managed-menu] button').first().click()
  await snap(page, `managed-report-${tag}`)
}

async function usage(page: Page, theme: Theme, lang: Lang, tag: string, quick = true) {
  await boot(page, { theme, lang })
  await store(page, 's.getState().closePendingDialog()')
  if (quick) {
    await page.locator('[data-usage-tier]').first().click()
    await snap(page, `usage-quick-${tag}`)
    // 重新载入再开详情：Esc 关浮层后焦点回到入口按钮，tooltip 会挂进详情图
    await boot(page, { theme, lang })
    await store(page, 's.getState().closePendingDialog()')
  }
  await store(page, 's.getState().openUsageDetail()')
  await snap(page, `usage-detail-${tag}`)
}

for (const theme of THEMES) {
  test.describe(`workbench ${theme}`, () => {
    test('shell + pending dock', ({ page }) => shell(page, theme, 'zh-CN', theme))
    test('canvas side-by-side + full width', ({ page }) => canvas(page, theme, 'zh-CN', theme))
    test('managed strip + report', ({ page }) => managed(page, theme, 'zh-CN', theme))
    test('usage quick + detail', ({ page }) => usage(page, theme, 'zh-CN', theme))
    test('model recovery strips', async ({ page }) => {
      await boot(page, { theme, query: 'mockrecovery' })
      await store(page, 's.getState().closePendingDialog()')
      await jump(page, 131)
      await snap(page, `recovery-model-${theme}`)
    })
  })
}

// fr：主界面浅色一套（长文案撑破布局最容易在法语暴露）
test.describe('workbench fr', () => {
  test('shell + pending dock', ({ page }) => shell(page, 'light', 'fr', 'fr'))
  test('canvas', ({ page }) => canvas(page, 'light', 'fr', 'fr'))
  test('managed', ({ page }) => managed(page, 'light', 'fr', 'fr'))
  test('usage detail', ({ page }) => usage(page, 'light', 'fr', 'fr', false))
})
