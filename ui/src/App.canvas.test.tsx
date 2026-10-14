// App 级画布与主区行为（ui-redesign 票 03，ADR 0088 / 0090；components B6 B7 B8，interaction §1 §3 §6 §12）。
// ⌥⌘B / ⌘\ / ⌘. / ⌘W、画布分割线（吸附关闭、推入全宽、双击还原）、窗口变窄的让位顺序、
// 布局持久化（hexagon.layout.canvas；专注模式不持久化）、SessionHost 恢复条在对话列顶部。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore, type WorkTab } from './store'
import { isMac } from './keymap'
import i18n from './i18n'

vi.mock('./components/PendingCards', () => ({ PendingDialog: () => null }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()
const innerWidth0 = window.innerWidth
const art: WorkTab = { id: 'art:spec.md', kind: 'artifact', title: 'spec.md', path: 'spec.md' }
const diff: WorkTab = { id: 'diff:spec', kind: 'diff', title: 'Diff', patchText: '' }

const mod = isMac ? { metaKey: true } : { ctrlKey: true }
const key = (k: string, mods: KeyboardEventInit = {}) => act(async () => {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...mods }))
})
const canvas = () => el.querySelector('[data-canvas]') as HTMLElement | null
const canvasVisible = () => !!canvas() && !canvas()!.hidden
const conversation = () => el.querySelector('[data-conversation]') as HTMLElement
const canvasHandle = () => el.querySelector('[role="separator"][aria-label="Canvas width"]') as HTMLElement
const savedCanvas = () => JSON.parse(localStorage.getItem('hexagon.layout.canvas') ?? 'null')
const setWidth = (w: number) => act(async () => {
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: w })
  window.dispatchEvent(new Event('resize'))
})

beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.removeItem('hexagon.layout.side')
  localStorage.removeItem('hexagon.layout.canvas')
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1440 })
  useUiStore.setState({
    projectRoot: null, projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null,
    canvasLayout: { width: null, full: false }, team: [], pending: [], artifacts: [], timeline: [], modalScope: 'workbench',
    intakeDraft: false, sideLayout: { width: 252, collapsed: false }, usageTotal: null, confirmReq: null, sessionSummary: null, sessionError: null,
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
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: innerWidth0 })
  localStorage.removeItem('hexagon.layout.side')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState(initial)
})

describe('画布开关与页签（B7，interaction §6 §12）', () => {
  it('⌥⌘B 无页签时打开空态，再按关闭；头部开关同步', async () => {
    await key('b', { ...mod, altKey: true })
    expect(canvasVisible()).toBe(true)
    expect(canvas()!.dataset.canvas).toBe('side')
    expect(el.querySelector('[data-canvas-empty]')!.textContent).toMatch(/Nothing open yet/)
    expect(el.querySelector('[data-header-canvas]')!.getAttribute('aria-pressed')).toBe('true')
    await key('b', { ...mod, altKey: true })
    expect(canvasVisible()).toBe(false)
  })

  it('时间线不是页签；打开产物进画布并聚焦当前页签，对话仍在', async () => {
    await act(async () => useUiStore.getState().openTab(art))
    expect(canvasVisible()).toBe(true)
    const tabs = [...canvas()!.querySelectorAll('[role="tab"]')]
    expect(tabs.map((t) => t.textContent)).toEqual(['spec.md'])
    expect(document.activeElement).toBe(tabs[0])
    expect(conversation().hidden).toBe(false)
    expect(el.querySelector('#composer-input')).toBeTruthy()
  })

  it('⌘W 关当前页签，关掉最后一个时画布关闭；画布关着时 ⌘W 不做事', async () => {
    await act(async () => { useUiStore.getState().openTab(art); useUiStore.getState().openTab(diff) })
    await key('w', mod)
    expect(useUiStore.getState().tabs.map((t) => t.id)).toEqual(['art:spec.md'])
    await key('w', mod)
    expect(useUiStore.getState().tabs).toEqual([])
    expect(canvasVisible()).toBe(false)
    await key('w', mod)
    expect(useUiStore.getState().canvasOpen).toBe(false)
  })

  it('⌘\\ 全宽 ↔ 并排：全宽时对话收起、最左出「对话」页签，点它回到并排；全宽状态持久化', async () => {
    await act(async () => useUiStore.getState().openTab(art))
    await key('\\', mod)
    expect(canvas()!.dataset.canvas).toBe('full')
    expect(conversation().hidden).toBe(true)
    expect(savedCanvas()).toEqual({ width: null, full: true })
    const back = el.querySelector('[data-canvas-conversation]') as HTMLButtonElement
    expect(back.textContent).toBe('Conversation')
    await act(async () => { back.click() })
    expect(canvas()!.dataset.canvas).toBe('side')
    expect(conversation().hidden).toBe(false)
    expect(savedCanvas()).toEqual({ width: null, full: false })
  })

  it('页签栏按钮 tooltip 读当前绑定：展开到全宽 ⌘\\、关闭画布 ⌥⌘B', async () => {
    await act(async () => useUiStore.getState().openTab(art))
    expect(el.querySelector('[data-canvas-expand]')!.getAttribute('aria-label')).toBe('Expand to full width')
    expect(el.querySelector('[data-canvas-close]')!.getAttribute('aria-label')).toBe('Close canvas')
  })

  it('侧栏「项目文件」以全宽打开，树在编辑器左侧（canvas#file）', async () => {
    vi.spyOn(api, 'listRepoDir').mockResolvedValue([])
    await act(async () => { (el.querySelector('[data-nav="files"]') as HTMLButtonElement).click() })
    expect(canvas()!.dataset.canvas).toBe('full')
    expect(el.querySelector('[data-canvas-tree]')).toBeTruthy()
  })
})

describe('画布分割线（B8，interaction §1）', () => {
  const W0 = Math.round(0.46 * (1440 - 252))
  const drag = async (to: number[]) => {
    const hit = canvasHandle().querySelector('.cursor-col-resize') as HTMLElement
    const x0 = 1440 - W0
    await act(async () => {
      hit.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, clientX: x0 }))
      for (const x of to) window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: x }))
    })
  }
  const up = () => act(async () => { window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true })) })

  beforeEach(async () => { await act(async () => useUiStore.getState().openTab(art)) })

  it('默认 46%，上限 窗口 − 侧栏 − 480；拖动跟手并持久化', async () => {
    expect(canvasHandle().getAttribute('aria-valuenow')).toBe(String(W0))
    expect(canvasHandle().getAttribute('aria-valuemax')).toBe(String(1440 - 252 - 480))
    await drag([1440 - W0 + 100])
    expect(el.querySelector('[data-slot="resize-badge"]')!.textContent).toBe(`${W0 - 100}px`)
    await up()
    expect(savedCanvas()).toEqual({ width: W0 - 100, full: false })
  })

  it('拖过下限 40px 吸附关闭，保留原宽', async () => {
    await drag([1440 - 200])
    expect(el.querySelector('[data-slot="resize-badge"]')!.textContent).toMatch(/Release to close the canvas/)
    await up()
    expect(useUiStore.getState().canvasOpen).toBe(false)
    expect(savedCanvas()).toEqual({ width: W0, full: false })
  })

  it('拖到上限再推 40px 进入全宽', async () => {
    await drag([1440 - 760])
    expect(el.querySelector('[data-slot="resize-badge"]')!.textContent).toMatch(/Release for full width/)
    await up()
    expect(canvas()!.dataset.canvas).toBe('full')
    expect(savedCanvas()).toEqual({ width: W0, full: true })
  })

  it('单击不做事（防误关），双击还原默认宽度', async () => {
    await act(async () => useUiStore.getState().setCanvasLayout({ width: 500, full: false }))
    const hit = canvasHandle().querySelector('.cursor-col-resize') as HTMLElement
    const click = () => act(async () => {
      hit.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, clientX: 940 }))
      window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }))
    })
    await click()
    await act(async () => { await new Promise((r) => setTimeout(r, 300)) })
    expect(useUiStore.getState().canvasOpen).toBe(true)
    expect(useUiStore.getState().canvasLayout.width).toBe(500)
    await click()
    await click()
    expect(useUiStore.getState().canvasLayout).toEqual({ width: null, full: false })
  })

  it('键盘：← 加宽 16（分割线左移）、↵ 关闭画布', async () => {
    const h = canvasHandle()
    await act(async () => { h.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true })) })
    expect(useUiStore.getState().canvasLayout.width).toBe(W0 + 16)
    await act(async () => { canvasHandle().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })) })
    expect(useUiStore.getState().canvasOpen).toBe(false)
  })
})

describe('窗口变窄的让位顺序（interaction §1：对话主区始终 ≥ 480）', () => {
  it('先压画布到 360 → 侧栏收成图标列（不改记忆）→ 画布改为 Sheet', async () => {
    await act(async () => useUiStore.getState().openTab(art))
    await setWidth(1100)
    expect(el.querySelector('[data-sidebar]')!.getAttribute('data-sidebar')).toBe('expanded')
    expect(canvas()!.style.width).toBe(`${1100 - 252 - 480}px`)
    await setWidth(1000)
    expect(el.querySelector('[data-sidebar]')!.getAttribute('data-sidebar')).toBe('collapsed')
    expect(canvas()!.dataset.canvas).toBe('side')
    expect(useUiStore.getState().sideLayout.collapsed).toBe(false)
    await setWidth(800)
    expect(canvas()!.dataset.canvas).toBe('sheet')
    expect(canvasHandle()).toBeNull()
    await setWidth(1440)
    expect(el.querySelector('[data-sidebar]')!.getAttribute('data-sidebar')).toBe('expanded')
  })
})

describe('专注模式 ⌘.（R-06）', () => {
  it('隐藏侧栏、关闭画布、隐藏节点轨；再按恢复原布局；不写布局记忆', async () => {
    await act(async () => useUiStore.getState().openTab(art))
    const before = [localStorage.getItem('hexagon.layout.side'), localStorage.getItem('hexagon.layout.canvas')]
    await key('.', mod)
    expect(el.querySelector('[data-sidebar]')).toBeNull()
    expect(canvasVisible()).toBe(false)
    expect(el.querySelector('[data-workbench]')!.hasAttribute('data-focus-mode')).toBe(true)
    expect(el.querySelector('#composer-input')).toBeTruthy()
    await key('.', mod)
    expect(el.querySelector('[data-sidebar]')!.getAttribute('data-sidebar')).toBe('expanded')
    expect(canvasVisible()).toBe(true)
    expect(useUiStore.getState().activeTab).toBe('art:spec.md')
    expect([localStorage.getItem('hexagon.layout.side'), localStorage.getItem('hexagon.layout.canvas')]).toEqual(before)
  })
})

describe('用量详情占满主区（interaction §10，ADR 0090）', () => {
  it('对话与画布让出主区，侧栏仍在；返回对话', async () => {
    await act(async () => { useUiStore.getState().openTab(art); useUiStore.getState().openUsageDetail() })
    expect(el.querySelector('[data-usage-view]')).toBeTruthy()
    expect(conversation().hidden).toBe(true)
    expect(canvasVisible()).toBe(false)
    expect(el.querySelector('[data-sidebar]')).toBeTruthy()
    await act(async () => { (el.querySelector('[data-usage-back]') as HTMLButtonElement).click() })
    expect(el.querySelector('[data-usage-view]')).toBeNull()
    expect(conversation().hidden).toBe(false)
    expect(canvasVisible()).toBe(true)
  })
})

describe('SessionHost 恢复条（C12a）随时间线迁入主区', () => {
  it('钉在对话列顶部（时间线与归来摘要之上），画布全宽时随对话一起隐藏', async () => {
    await act(async () => useUiStore.setState({ sessionError: 'boom' }))
    const strip = el.querySelector('[data-session-recovery]') as HTMLElement
    expect(conversation().contains(strip)).toBe(true)
    expect(strip.parentElement!.firstElementChild).toBe(strip)
    const timelineSlot = strip.nextElementSibling as HTMLElement
    expect(timelineSlot.compareDocumentPosition(el.querySelector('#composer-input')!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    await act(async () => { useUiStore.getState().openTab(art); useUiStore.getState().toggleCanvasFull() })
    expect(conversation().hidden).toBe(true)
    expect(canvas()!.contains(strip)).toBe(false)
  })
})
