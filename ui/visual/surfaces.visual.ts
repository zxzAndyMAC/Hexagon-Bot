// 截图回归 · 入口与覆盖面（票 ui-redesign 17）：启动页、向导各步、设置列表与详情、反馈层。深浅两套 + fr 主界面。
import { test, type Page } from '@playwright/test'
import { boot, settle, snap, store, tr, THEMES, type Lang, type Theme } from './helpers.ts'

const IDEA = '一个离线可用的待办清单 PWA，单用户，数据存在本机'

/** 向导：填满当前步必填项后点「下一步」，每步一张。步数按 mock 走到确认页为止。 */
async function wizard(page: Page, theme: Theme, lang: Lang, tag: string) {
  await boot(page, { theme, lang, query: 'launcher' })
  await snap(page, `entry-launcher-${tag}`)
  await page.locator('[data-blank-new]').click()
  await snap(page, `wizard-step1-${tag}`)
  const next = page.locator('[data-wizard-status] ~ button').last()
  for (let i = 2; i <= 9; i++) {
    if (!(await next.isEnabled())) {
      const ta = page.locator('[data-wizard] textarea').first()
      if (await ta.count()) await ta.fill(IDEA)
      const inputs = page.locator('[data-wizard] input[type="text"], [data-wizard] input:not([type])')
      for (let k = 0; k < (await inputs.count()); k++) {
        const el = inputs.nth(k)
        if (!(await el.inputValue())) await el.fill(k === 0 ? '/tmp/empty-todo-pwa' : 'todo-pwa')
      }
    }
    // 确认页（最后一步）主按钮是「启动」，不点
    if ((await next.textContent())?.trim() === (await tr(page, 'wizard.launch'))) break
    await next.click()
    await settle(page, 1500) // mock 的 AI 起草有投递延时
    await snap(page, `wizard-step${i}-${tag}`)
  }
}

async function wizardNoProvider(page: Page, theme: Theme) {
  await boot(page, { theme, query: 'launcher&mocknoprovider' })
  await page.locator('[data-blank-new]').click()
  await snap(page, `wizard-noprovider-${theme}`)
}

async function settings(page: Page, theme: Theme, lang: Lang, tag: string, sections: string[]) {
  await boot(page, { theme, lang })
  await store(page, 's.getState().closePendingDialog()')
  const label = await tr(page, 'sidebar.settings')
  await page.locator(`button[aria-label^="${label}"]`).first().click()
  await page.locator('[data-settings]').waitFor()
  await snap(page, `settings-general-${tag}`)
  for (const id of sections) {
    const nav = await tr(page, `settings.nav_${id}`)
    await page.locator('[data-settings] nav button, [data-settings] nav a').filter({ hasText: nav }).first().click()
    await snap(page, `settings-${id}-${tag}`)
  }
}

async function feedback(page: Page, theme: Theme) {
  await boot(page, { theme })
  await store(page, 's.getState().closePendingDialog()')
  await store(page, `s.getState().askConfirm({ title: '移除 MCP 服务 figma？', body: '只从 ~/.hexagon/mcp.json 删除这条配置，不会卸载程序。', confirmLabel: '移除', danger: true, run: () => {} })`)
  await snap(page, `feedback-confirm-danger-${theme}`)
  // 规范样张页：toast / 空态 / 故障与恢复（静态样张，不受 Sonner 寿命计时影响）
  await boot(page, { theme, query: 'specimen' })
  for (const sec of ['toast', 'empty', 'errors']) {
    await page.evaluate((id) => document.getElementById(`fb-${id}`)?.scrollIntoView(), sec)
    await snap(page, `feedback-${sec}-${theme}`)
  }
}

for (const theme of THEMES) {
  test.describe(`surfaces ${theme}`, () => {
    test('launcher + wizard steps', ({ page }) => wizard(page, theme, 'zh-CN', theme))
    test('wizard without provider', ({ page }) => wizardNoProvider(page, theme))
    test('settings list + detail', ({ page }) => settings(page, theme, 'zh-CN', theme, ['models', 'perms']))
    test('feedback layer', ({ page }) => feedback(page, theme))
  })
}

test.describe('surfaces fr', () => {
  test('launcher + wizard steps', ({ page }) => wizard(page, 'light', 'fr', 'fr'))
  test('settings', ({ page }) => settings(page, 'light', 'fr', 'fr', ['models']))
})
