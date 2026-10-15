// App 级外壳行为（ui-redesign 票 02，ADR 0088；components B1/B4/B5/B8，interaction §1 §10 §12）。
// ⌘B 开关侧栏、分割线单击 / 双击 / 拖动吸附 / 键盘、布局持久化、触顶横幅、全员休眠二次确认。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore } from './store'
import { isMac } from './keymap'
import i18n from './i18n'

// 待决浮层的进出场动画在 happy-dom 里卸载时抛 AbortError；本文件只测外壳，浮层换桩。
vi.mock('./components/PendingDock', () => ({ PendingDock: () => null }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()


const sidebar = () => el.querySelector('[data-sidebar]') as HTMLElement
const handle = () => el.querySelector('[role="separator"][aria-label]') as HTMLElement
const saved = () => JSON.parse(localStorage.getItem('hexagon.layout.side') ?? 'null')
const key = (k: string, mods: KeyboardEventInit = {}) => act(async () => {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, ...mods }))
})
const mod = isMac ? { metaKey: true } : { ctrlKey: true }

beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.removeItem('hexagon.layout.side')
  useUiStore.setState({
    projectRoot: null, projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null, team: [], pending: [],
    timeline: [], modalScope: 'workbench', intakeDraft: false, sideLayout: { width: 252, collapsed: false }, usageTotal: null, confirmReq: null,
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
  vi.useRealTimers()
  localStorage.removeItem('hexagon.layout.side')
  useUiStore.setState(initial)
})

describe('⌘B 与侧栏分割线（ADR 0088：⌘B 由右栏改为侧栏）', () => {
  it('⌘B 在展开与 56 宽图标列之间切换并持久化', async () => {
    expect(sidebar().dataset.sidebar).toBe('expanded')
    await key('b', mod)
    expect(sidebar().dataset.sidebar).toBe('collapsed')
    expect(saved()).toEqual({ width: 252, collapsed: true })
    await key('b', mod)
    expect(sidebar().dataset.sidebar).toBe('expanded')
  })

  it('图标列保留待决红点与团队头像，图标有可读名', async () => {
    await act(async () => useUiStore.setState({
      pending: [{ id: 'q1', kind: 'permission', agent_id: 'a1', payload: {}, state: 'queued' } as never],
      team: [{ id: 'a1', role: 'QA', model_slot: null, status: 'active', avatar_hash: null, model: null, duty: null }],
      sideLayout: { width: 252, collapsed: true },
    }))
    expect(sidebar().querySelector('[data-pending-dot]')).toBeTruthy()
    expect(sidebar().querySelector('[data-member="a1"]')).toBeTruthy()
    expect(sidebar().querySelector('[data-rail="pending"] button')!.getAttribute('aria-label')).toMatch(/\d+ pending/)
  })

  it('单击分割线 = ⌘B，双击还原默认宽度', async () => {
    vi.useFakeTimers()
    await act(async () => useUiStore.getState().setSideLayout({ width: 320, collapsed: false }))
    const hit = () => handle().querySelector('.cursor-col-resize') as HTMLElement
    const click = async () => act(async () => {
      hit().dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, clientX: 320 }))
      window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, clientX: 320 }))
    })
    await click()
    await act(async () => { vi.advanceTimersByTime(300) })
    expect(useUiStore.getState().sideLayout).toEqual({ width: 320, collapsed: true })
    await click()
    await act(async () => { vi.advanceTimersByTime(300) })
    expect(useUiStore.getState().sideLayout.collapsed).toBe(false)
    await click()
    await click()
    await act(async () => { vi.advanceTimersByTime(300) })
    expect(useUiStore.getState().sideLayout).toEqual({ width: 252, collapsed: false })
  })

  it('拖动跟手、触限停住，拖过下限 40px 吸附收起并保留原宽', async () => {
    const hit = handle().querySelector('.cursor-col-resize') as HTMLElement
    await act(async () => {
      hit.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, clientX: 252 }))
      window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: 300 }))
    })
    expect(el.querySelector('[data-slot="resize-badge"]')!.textContent).toBe('300px')
    await act(async () => { window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: 500 })) })
    expect(handle().dataset.edge).toBe('max')
    expect(el.querySelector('[data-slot="resize-badge"]')!.textContent).toMatch(/360px · max/)
    await act(async () => { window.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: 100 })) })
    expect(el.querySelector('[data-slot="resize-badge"]')!.textContent).toMatch(/Release to collapse/)
    await act(async () => { window.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, clientX: 100 })) })
    expect(saved()).toEqual({ width: 252, collapsed: true })
  })

  it('键盘：←→ 16、Shift 64、Home/End、↵ 收展；role=separator 读宽度', async () => {
    const h = handle()
    expect(h.getAttribute('aria-valuenow')).toBe('252')
    const press = (k: string, shiftKey = false) => act(async () => {
      h.dispatchEvent(new KeyboardEvent('keydown', { key: k, shiftKey, bubbles: true }))
    })
    await press('ArrowRight')
    expect(useUiStore.getState().sideLayout.width).toBe(268)
    await press('ArrowLeft', true)
    expect(useUiStore.getState().sideLayout.width).toBe(204)
    await press('End')
    expect(handle().getAttribute('aria-valuenow')).toBe('360')
    await press('Home')
    expect(useUiStore.getState().sideLayout.width).toBe(200)
    await press('Enter')
    expect(useUiStore.getState().sideLayout.collapsed).toBe(true)
    expect(saved()).toEqual({ width: 200, collapsed: true })
  })
})

describe('用量触顶横幅（B4，usage_summary）', () => {
  const total = (spent_mc: number) => ({ reserved_mc: 0, spent_mc, limit_cents: 10000, tokens: 0, unknown_requests: 0, legacy_unknown_records: 0, unknown_token_records: 0 })
  it('未触顶不出横幅；触顶时输入条上方出横幅，「查看用量」打开用量详情', async () => {
    await act(async () => useUiStore.setState({ usageTotal: total(9_000_000) }))
    expect(el.querySelector('[data-cap-banner]')).toBeNull()
    await act(async () => useUiStore.setState({ usageTotal: total(10_000_000) }))
    const banner = el.querySelector('[data-cap-banner]') as HTMLElement
    expect(banner.textContent).toMatch(/new model requests will be refused/)
    expect(banner.nextElementSibling).not.toBeNull()
    const view = [...banner.querySelectorAll('button')].find((b) => b.textContent === 'View usage')!
    await act(async () => { view.click() })
    // ADR 0090（票 03）：用量详情由中栏页签改为占满主区的视图。
    expect(useUiStore.getState().usageView).toBe(true)
    expect(el.querySelector('[data-usage-view]')).toBeTruthy()
  })
})

describe('头部 ⋯ 全员休眠（票 02：仍要二次确认）', () => {
  it('选中后先挂确认，不直接休眠', async () => {
    const sleep = vi.spyOn(api, 'sleepAll').mockResolvedValue()
    const more = el.querySelector('[data-header-more]') as HTMLElement
    await act(async () => { more.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, pointerType: 'mouse' })) })
    const item = [...document.querySelectorAll('[role="menuitem"]')].find((n) => /Put everyone to sleep/.test(n.textContent ?? '')) as HTMLElement
    await act(async () => { item.click() })
    expect(sleep).not.toHaveBeenCalled()
    expect(useUiStore.getState().confirmReq?.title).toMatch(/whole team/)
  })
})
