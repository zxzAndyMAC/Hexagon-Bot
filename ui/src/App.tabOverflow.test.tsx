// 画布页签溢出（components A22「溢出」，owner-followups-1015 #06）。
// 新打开 / 激活的页签滚入视区；竖向滚轮转横向；「全部页签」菜单列全部（当前 ✓，>10 条出搜索）并能跳转；
// 固定页签（产物 / 项目文件）钉在滚动区外左侧；nextTab / prevTab 键位按可见顺序循环。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore, type WorkTab } from './store'
import { formatBinding, isMac, matches, resetBinding } from './keymap'
import { adjacentTab, pinnedFirst, revealDelta } from './tabActions'
import i18n from './i18n'

vi.mock('./components/PendingDock', () => ({ PendingDock: () => null }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()
const fileTab = (n: number): WorkTab => ({ id: `file:src/f${n}.ts`, kind: 'file', title: `f${n}.ts`, path: `src/f${n}.ts` })
const artifacts: WorkTab = { id: 'artifacts', kind: 'artifacts', title: 'Artifacts' }
const files: WorkTab = { id: 'files', kind: 'files', title: 'Files' }

const scroller = () => el.querySelector('[data-canvas-tab-scroller]') as HTMLElement
const pinned = () => el.querySelector('[data-canvas-tab-pinned]') as HTMLElement | null
const tabEl = (id: string) => el.querySelector(`[data-canvas-tab="${id}"]`) as HTMLElement
const allTabsButton = () => el.querySelector('[data-canvas-all-tabs]') as HTMLElement
const menu = () => document.querySelector('[data-slot="dropdown-menu-content"]') as HTMLElement | null
const menuRows = () => [...menu()!.querySelectorAll<HTMLElement>('[data-all-tabs-item]')]
const openTabs = (tabs: WorkTab[], active: string) => act(async () => {
  useUiStore.setState({ tabs, activeTab: active, canvasOpen: true })
})
const openMenu = () => act(async () => {
  allTabsButton().dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' }))
})
const press = (init: KeyboardEventInit) => act(async () => {
  window.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }))
})
const mod = isMac ? { metaKey: true } : { ctrlKey: true }

/** happy-dom 不排版：给滚动区与页签钉矩形（页签宽 120 依次排开，视区 0–300）。 */
function fakeLayout() {
  const s = scroller()
  vi.spyOn(s, 'getBoundingClientRect').mockImplementation(() => DOMRect.fromRect({ x: 0, y: 0, width: 300, height: 30 }))
  const ids = () => [...s.querySelectorAll<HTMLElement>('[data-canvas-tab]')]
  for (const node of ids()) {
    vi.spyOn(node, 'getBoundingClientRect').mockImplementation(() => {
      const i = ids().indexOf(node)
      return DOMRect.fromRect({ x: i * 120 - s.scrollLeft, y: 0, width: 120, height: 30 })
    })
  }
}

beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState({
    projectRoot: null, projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null,
    canvasLayout: { width: null, full: false }, team: [], pending: [], artifacts: [], timeline: [], modalScope: 'workbench',
    intakeDraft: false, usageTotal: null, confirmReq: null, sessionSummary: null, sessionError: null, fileEdits: {},
  })
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => root.render(<App />))
})

afterEach(async () => {
  await act(async () => root.unmount())
  el.remove()
  vi.restoreAllMocks()
  resetBinding('nextTab')
  resetBinding('prevTab')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState(initial)
})

describe('revealDelta：把页签拉进视区要滚多少', () => {
  const box = (left: number, right: number) => ({ left, right })
  it('已在视区内 → 0', () => expect(revealDelta(box(0, 300), box(120, 240))).toBe(0))
  it('右侧露不全 → 正数（右边对齐视区右缘）', () => expect(revealDelta(box(0, 300), box(240, 360))).toBe(60))
  it('左侧露不全 → 负数（左边对齐视区左缘）', () => expect(revealDelta(box(0, 300), box(-50, 70))).toBe(-50))
  it('比视区还宽 → 左边对齐', () => expect(revealDelta(box(0, 100), box(40, 240))).toBe(40))
})

describe('tabActions：可见顺序与上 / 下一个', () => {
  const a = fileTab(1), b = fileTab(2)
  it('固定页签排在最前，其余保持原序', () => {
    expect(pinnedFirst([a, files, b, artifacts]).map((t) => t.id)).toEqual(['files', 'artifacts', a.id, b.id])
  })
  it('按可见顺序循环', () => {
    const tabs = [a, files, b]
    expect(adjacentTab(tabs, 'files', 1)).toBe(a.id)
    expect(adjacentTab(tabs, b.id, 1)).toBe('files')
    expect(adjacentTab(tabs, 'files', -1)).toBe(b.id)
  })
  it('只有一个或当前不存在 → null', () => {
    expect(adjacentTab([a], a.id, 1)).toBeNull()
    expect(adjacentTab([a, b], 'nope', 1)).toBeNull()
  })
})

describe('页签溢出（A22）', () => {
  it('新打开的文件页签总是滚进可见区', async () => {
    const tabs = Array.from({ length: 6 }, (_, i) => fileTab(i))
    await openTabs(tabs, tabs[0].id)
    fakeLayout()
    expect(scroller().scrollLeft).toBe(0)
    await act(async () => { useUiStore.getState().openTab(fileTab(6)) })
    fakeLayout()
    // 再触发一次（新页签挂载后才有矩形）：激活页签变化即滚入
    await act(async () => { useUiStore.getState().setActiveTab(tabs[0].id) })
    await act(async () => { useUiStore.getState().setActiveTab(fileTab(6).id) })
    const r = tabEl(fileTab(6).id).getBoundingClientRect()
    expect(r.right).toBeLessThanOrEqual(300)
    expect(r.left).toBeGreaterThanOrEqual(0)
  })

  it('竖向滚轮在页签栏上转成横向滚动', async () => {
    const tabs = Array.from({ length: 6 }, (_, i) => fileTab(i))
    await openTabs(tabs, tabs[0].id)
    await act(async () => { scroller().dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: 80 })) })
    expect(scroller().scrollLeft).toBe(80)
  })

  it('页签压缩下限 96：省略号 + title 全名', async () => {
    await openTabs([fileTab(1)], fileTab(1).id)
    expect(tabEl(fileTab(1).id).className).toMatch(/min-w-\[96px\]/)
    expect(tabEl(fileTab(1).id).querySelector('[role="tab"]')!.getAttribute('title')).toBe('src/f1.ts')
  })

  it('固定页签（产物 / 项目文件）钉在滚动区外左侧', async () => {
    await openTabs([fileTab(1), artifacts, fileTab(2), files], fileTab(1).id)
    expect(pinned()).not.toBeNull()
    expect([...pinned()!.querySelectorAll('[data-canvas-tab]')].map((n) => (n as HTMLElement).dataset.canvasTab)).toEqual(['artifacts', 'files'])
    expect(scroller().querySelector('[data-canvas-tab="artifacts"]')).toBeNull()
    // 钉住的组不在滚动容器里（滚动不带走它），且排在它前面
    expect(scroller().contains(pinned())).toBe(false)
    expect(pinned()!.compareDocumentPosition(scroller()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('openTab 把固定页签插到固定组末尾，store 顺序与可见顺序一致', async () => {
    await openTabs([fileTab(1)], fileTab(1).id)
    await act(async () => { useUiStore.getState().openCanvas('files') })
    await act(async () => { useUiStore.getState().openTab(fileTab(2)) })
    await act(async () => { useUiStore.getState().openCanvas('artifacts') })
    expect(useUiStore.getState().tabs.map((t) => t.id)).toEqual(['files', 'artifacts', fileTab(1).id, fileTab(2).id])
  })
})

describe('「全部页签」菜单', () => {
  it('列出全部页签（可见顺序），当前打勾，点了跳过去', async () => {
    await openTabs([fileTab(1), files, fileTab(2)], fileTab(2).id)
    expect(allTabsButton().getAttribute('aria-label')).toBe('All tabs')
    await openMenu()
    expect(menuRows().map((r) => r.dataset.allTabsItem)).toEqual(['files', fileTab(1).id, fileTab(2).id])
    expect(menuRows().map((r) => r.getAttribute('aria-checked'))).toEqual(['false', 'false', 'true'])
    expect(menu()!.querySelector('[data-all-tabs-search]')).toBeNull()
    await act(async () => { menuRows()[1].click() })
    expect(useUiStore.getState().activeTab).toBe(fileTab(1).id)
  })

  it('>10 条时顶部出搜索，按名字 / 路径过滤', async () => {
    const tabs = Array.from({ length: 11 }, (_, i) => fileTab(i))
    await openTabs(tabs, tabs[0].id)
    await openMenu()
    const search = menu()!.querySelector('[data-all-tabs-search]') as HTMLInputElement
    expect(search).not.toBeNull()
    expect(search.getAttribute('aria-label')).toBe('Search tabs')
    expect(document.activeElement).toBe(search)
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      set.call(search, 'f10')
      search.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(menuRows().map((r) => r.dataset.allTabsItem)).toEqual([fileTab(10).id])
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      set.call(search, 'zzz')
      search.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(menuRows()).toHaveLength(0)
    expect(menu()!.textContent).toMatch(/No matching tabs/)
  })

  it('菜单底部写上 / 下一个页签的当前绑定', async () => {
    await openTabs([fileTab(1), fileTab(2)], fileTab(1).id)
    await openMenu()
    expect(menu()!.textContent).toContain(formatBinding('mod+shift+]'))
    expect(menu()!.textContent).toContain(formatBinding('mod+shift+['))
  })

  it('中文：按钮名「全部页签」', async () => {
    await act(async () => { await i18n.changeLanguage('zh-CN') })
    await openTabs([fileTab(1)], fileTab(1).id)
    expect(allTabsButton().getAttribute('aria-label')).toBe('全部页签')
  })
})

describe('nextTab / prevTab 键位', () => {
  it('默认 mod+shift+] / mod+shift+[，Shift 改了 key（} / {）也认', () => {
    const ev = (key: string, code: string) => new KeyboardEvent('keydown', { key, code, shiftKey: true, ...mod })
    expect(matches(ev('}', 'BracketRight'), 'mod+shift+]')).toBe(true)
    expect(matches(ev('{', 'BracketLeft'), 'mod+shift+[')).toBe(true)
    expect(matches(ev('}', 'BracketRight'), 'mod+shift+[')).toBe(false)
  })

  it('按可见顺序切到下一个 / 上一个页签，并循环', async () => {
    await openTabs([fileTab(1), files, fileTab(2)], 'files')
    await press({ key: '}', code: 'BracketRight', shiftKey: true, ...mod })
    expect(useUiStore.getState().activeTab).toBe(fileTab(1).id)
    await press({ key: '}', code: 'BracketRight', shiftKey: true, ...mod })
    expect(useUiStore.getState().activeTab).toBe(fileTab(2).id)
    await press({ key: '}', code: 'BracketRight', shiftKey: true, ...mod })
    expect(useUiStore.getState().activeTab).toBe('files')
    await press({ key: '{', code: 'BracketLeft', shiftKey: true, ...mod })
    expect(useUiStore.getState().activeTab).toBe(fileTab(2).id)
  })

  it('画布关着时不切', async () => {
    await openTabs([fileTab(1), fileTab(2)], fileTab(1).id)
    await act(async () => { useUiStore.setState({ canvasOpen: false }) })
    await press({ key: '}', code: 'BracketRight', shiftKey: true, ...mod })
    expect(useUiStore.getState().activeTab).toBe(fileTab(1).id)
  })

  it('命令面板有下一个 / 上一个页签，并显示绑定', async () => {
    await openTabs([fileTab(1), fileTab(2)], fileTab(1).id)
    await press({ key: 'k', ...mod })
    const items = [...document.querySelectorAll('[data-command-palette] [cmdk-item]')].map((n) => n.textContent ?? '')
    expect(items.find((x) => x.startsWith('Next tab'))).toContain(formatBinding('mod+shift+]'))
    expect(items.find((x) => x.startsWith('Previous tab'))).toContain(formatBinding('mod+shift+['))
  })
})
