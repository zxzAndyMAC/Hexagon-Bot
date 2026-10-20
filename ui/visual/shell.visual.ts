// 截图回归 · 工作台面（票 ui-redesign 17）：外壳、待决坞、画布、托管、用量、恢复。深浅两套 + fr 主界面。
import { expect, test, type Page } from '@playwright/test'
import { boot, settle, snap, store, THEMES, type Lang, type Theme } from './helpers.ts'

/** 时间线跳到某条事件并藏起钉住条（与票 16 截图同一取景）。 */
async function jump(page: Page, seq: number) {
  await page.addStyleTag({ content: '.tl-pin { display: none !important }' })
  // 反复跳到落点不再变：第一次跳转时虚拟列表还在量行高，落点随量测进度漂
  // （同一基线前后差 2% 像素；固定跳两次仍偶发漂，票 17 实测）。
  // 只要求「连续两次读数相同」仍会偶发差一行（约 32px，recovery-model-dark / managed-strip-fr，
  // 2026-10-17 合并 review 修复后全量门禁实测）：量测在两次读数之间暂停又继续。
  // 现在读数含 scroller 偏移 + 全部已渲染行的高度签名，连续三次相同才算稳。
  const probe = () => page.evaluate(() => [...document.querySelectorAll('[data-virtuoso-scroller]')]
    .map((e) => `${e.scrollTop}:${e.scrollHeight}:${[...e.querySelectorAll('[data-item-index]')].map((r) => (r as HTMLElement).offsetHeight).join('.')}`)
    .join('|'))
  let prev: string | null = null
  let same = 0
  for (let i = 0; i < 12 && same < 2; i++) {
    await page.evaluate((d) => window.dispatchEvent(new CustomEvent('hexagon:timeline-jump', { detail: d })), seq)
    await settle(page, 600)
    const now = await probe()
    same = now === prev ? same + 1 : 0
    prev = now
  }
}

async function shell(page: Page, theme: Theme, lang: Lang, tag: string) {
  await boot(page, { theme, lang })
  await snap(page, `shell-dock-open-${tag}`)
  await store(page, 's.getState().closePendingDialog()')
  await snap(page, `shell-dock-collapsed-${tag}`)
  // owner-followups-1015 #26：不可逆卡（推送）的红 30% 环线贴合坞的 16 圆角——曾画在直角卡盒上、四角戳出坞外。
  // 走托管报告「定位到卡」同一事件翻页（同时展开坞），不绑页码；放在收起图之后，
  // 免得坞变高后时间线滚动落点漂移带进 collapsed 基线（首版放在前面，collapsed-light 差 12px）。
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('hexagon:locate-pending', { detail: 'q-pub' })))
  // 只拍坞本身（外留 12px 看四角是否戳出）：坞变高后上方时间线会重新量行，整屏拍会把这段量测抖动带进基线。
  const dock = page.locator('.dock')
  await page.locator('.dock-card[data-danger]').waitFor()
  await settle(page, 250)
  const box = await dock.boundingBox()
  if (!box) throw new Error('dock not rendered')
  await expect(page).toHaveScreenshot(`shell-dock-danger-${tag}.png`,
    { clip: { x: box.x - 12, y: box.y - 12, width: box.width + 24, height: box.height + 24 } })
}

async function canvas(page: Page, theme: Theme, lang: Lang, tag: string) {
  await boot(page, { theme, lang })
  await store(page, 's.getState().closePendingDialog(); s.getState().openCanvas("artifacts")')
  await snap(page, `canvas-side-${tag}`)
  await store(page, 's.getState().toggleCanvasFull()')
  await snap(page, `canvas-full-${tag}`)
  // owner-followups-1015 #05（A16b）：页签右键菜单；唯一页签 → 关闭其他 / 关闭右侧禁用写原因，列表页签无复制路径。
  await page.locator('[data-canvas-tab]').first().click({ button: 'right' })
  await page.locator('[data-slot="context-menu-content"]').waitFor()
  await snap(page, `canvas-tab-menu-${tag}`)
  // owner-followups-1015 #06（A22「溢出」）：页签放不下 → 压到 96 + 横向滚动，最新打开的滚进视区，固定页签钉左；
  // 「全部页签」菜单 >10 条出搜索（打开即聚焦）。
  await page.keyboard.press('Escape')
  await store(page, `const st = s.getState(); st.toggleCanvasFull(); for (const p of ${JSON.stringify(OVERFLOW_PATHS)}) st.openTab({ id: 'art:' + p, kind: 'artifact', title: p, path: p })`)
  await snap(page, `canvas-tab-overflow-${tag}`)
  await page.locator('[data-canvas-all-tabs]').click()
  await page.locator('[data-all-tabs-search]').waitFor()
  await snap(page, `canvas-all-tabs-${tag}`)
  // owner-followups-1015 #07（A16b 文件树行）：文件行右键 → 打开 · 插入到输入条 · 复制路径 · 在访达中显示（macOS 基线）。
  await page.keyboard.press('Escape')
  await store(page, 's.getState().openCanvas("files")')
  const readme = page.locator('[role="treeitem"]', { hasText: 'README.md' })
  await readme.click({ button: 'right' })
  await page.locator('[data-slot="context-menu-content"]').waitFor()
  await snap(page, `file-tree-menu-${tag}`)
}

/** 12 个产物页签 + 固定「产物」清单 = 13 条（>10 出搜索）；最后一个是 mock 里有内容的产物。 */
const OVERFLOW_PATHS = [
  'specs/prd.md', 'ui/screens.md', 'api/spec.md', 'tests/qa-report.md', 'reviews/be-spec.md', 'docs/impl-notes.md',
  'docs/markdown-demo.md', 'proposals/p2.md', 'scripts/seed.py', 'src/recipes.ts', 'crates/core/src/api.rs', 'docs/arch.md',
]

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
