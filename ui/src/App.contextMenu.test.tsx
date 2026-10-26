// 右键菜单基础 + 画布页签菜单（components A16b，owner-followups-1015 #05）。
// 菜单项与顺序、种类不适用不渲染、此刻做不了禁用写原因、右键不改选中（只出焦点环）、
// Shift+F10 / Menu 键打开、关闭后焦点回触发处、行右侧显示当前绑定、动作进命令面板。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore, type WorkTab } from './store'
import { formatBinding, isMac, resetBinding, setBinding } from './keymap'
import i18n from './i18n'

vi.mock('./components/PendingDock', () => ({ PendingDock: () => null }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()
const art: WorkTab = { id: 'art:spec.md', kind: 'artifact', title: 'spec.md', path: 'docs/spec.md' }
const file: WorkTab = { id: 'file:src/main.rs', kind: 'file', title: 'main.rs', path: 'src/main.rs' }
const agent: WorkTab = { id: 'agent:a1', kind: 'agent', title: 'Coder', agentId: 'a1', role: 'Coder' }

const tabEl = (id: string) => el.querySelector(`[data-canvas-tab="${id}"]`) as HTMLElement
const tabButton = (id: string) => tabEl(id).querySelector('[role="tab"]') as HTMLButtonElement
const menu = () => document.querySelector('[data-slot="context-menu-content"]') as HTMLElement | null
const rows = () => [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')]
const row = (label: string) => rows().find((r) => r.textContent!.startsWith(label))!
const rightClick = (target: Element) => act(async () => {
  const r = target.getBoundingClientRect()
  target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: r.left + 4, clientY: r.top + 4 }))
})
const keyOn = (target: Element, init: KeyboardEventInit) => act(async () => {
  target.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }))
})
const pick = (label: string) => act(async () => { row(label).click() })
const openTabs = (tabs: WorkTab[], active: string) => act(async () => {
  useUiStore.setState({ tabs, activeTab: active, canvasOpen: true })
})

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
  vi.spyOn(api, 'openWithCandidates').mockResolvedValue([])
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
  resetBinding('closeOtherTabs')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState(initial)
})

describe('画布页签右键菜单（A16b 第一批）', () => {
  it('产物页签：关闭 · 关闭其他 · 关闭右侧 · 复制路径，依此顺序', async () => {
    await openTabs([art, file, agent], art.id)
    await rightClick(tabEl(art.id))
    expect(menu()).not.toBeNull()
    // #31：文件 / 产物页签末尾加「打开方式 ▸」子菜单触发行（也是 menuitem）。
    expect(rows().map((r) => r.dataset.ctxItem)).toEqual(['close', 'closeOthers', 'closeRight', 'copyPath', 'openWith'])
  })

  it('Agent 页签没有「复制路径」（种类不适用 → 不渲染）', async () => {
    await openTabs([art, agent], art.id)
    await rightClick(tabEl(agent.id))
    expect(rows().map((r) => r.dataset.ctxItem)).toEqual(['close', 'closeOthers', 'closeRight'])
  })

  it('此刻做不了 → 禁用并写原因：最右页签「右侧没有页签」，唯一页签「没有其他页签」', async () => {
    await openTabs([art], art.id)
    await rightClick(tabEl(art.id))
    expect(row('Close to the right').getAttribute('aria-disabled')).toBe('true')
    expect(row('Close to the right').textContent).toContain('No tabs to the right')
    expect(row('Close others').getAttribute('aria-disabled')).toBe('true')
    expect(row('Close others').textContent).toContain('No other tabs')
  })

  it('中文：禁用原因写「右侧没有页签」', async () => {
    await act(async () => { await i18n.changeLanguage('zh-CN') })
    await openTabs([art], art.id)
    await rightClick(tabEl(art.id))
    expect(rows().map((r) => r.textContent)).toEqual(['关闭' + formatBinding('mod+W'), '关闭其他没有其他页签', '关闭右侧右侧没有页签', '复制路径', '打开方式'])  // #31 子菜单触发行
  })

  it('右键不改选中：非当前页签不激活，只给目标出焦点环标记', async () => {
    await openTabs([art, file], art.id)
    await rightClick(tabEl(file.id))
    expect(useUiStore.getState().activeTab).toBe(art.id)
    expect(tabEl(file.id).dataset.state).toBe('open')
    expect(tabEl(art.id).dataset.state).toBe('closed')
  })

  it('关闭其他：只留目标并激活它', async () => {
    await openTabs([art, file, agent], agent.id)
    await rightClick(tabEl(file.id))
    await pick('Close others')
    expect(useUiStore.getState().tabs.map((t) => t.id)).toEqual([file.id])
    expect(useUiStore.getState().activeTab).toBe(file.id)
  })

  it('关闭右侧：只关目标右边的页签；当前页签不在其中时不动', async () => {
    await openTabs([art, file, agent], art.id)
    await rightClick(tabEl(file.id))
    await pick('Close to the right')
    expect(useUiStore.getState().tabs.map((t) => t.id)).toEqual([art.id, file.id])
    expect(useUiStore.getState().activeTab).toBe(art.id)
  })

  it('关闭复用 closeTab；行右侧显示当前绑定 ⌘W', async () => {
    await openTabs([art, file], art.id)
    await rightClick(tabEl(file.id))
    expect(row('Close').textContent).toContain(formatBinding('mod+W'))
    await pick('Close')
    expect(useUiStore.getState().tabs.map((t) => t.id)).toEqual([art.id])
  })

  it('未绑定的动作行右侧不显示键位；绑定后显示', async () => {
    await openTabs([art, file], art.id)
    await rightClick(tabEl(art.id))
    expect(row('Close others').querySelector('[data-ctx-hint]')).toBeNull()
    await keyOn(menu()!, { key: 'Escape' })
    setBinding('closeOtherTabs', 'alt+mod+shift+o')
    await rightClick(tabEl(art.id))
    expect(row('Close others').querySelector('[data-ctx-hint]')!.textContent).toBe(formatBinding('alt+mod+shift+o'))
  })

  // 行为变更（票 29，2026-10-20 负责人裁决）：页签「复制路径」写绝对路径（项目根 + 相对路径）；
  // 原断言是项目相对路径 'src/main.rs'。文件树行的「复制路径」不在这次裁决内，仍是相对路径。
  it('复制路径：写入页签的绝对路径并出回执', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    useUiStore.setState({ projectRoot: '/Users/me/todo' })
    await openTabs([art, file], art.id)
    await rightClick(tabEl(file.id))
    await pick('Copy path')
    expect(writeText).toHaveBeenCalledWith('/Users/me/todo/src/main.rs')
    expect(useUiStore.getState().toasts.at(-1)?.text).toBe('Path copied')
  })

  it('复制路径：copyPath 键位作用于当前页签时同样写绝对路径（票 29）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    useUiStore.setState({ projectRoot: '/Users/me/todo/' })
    await openTabs([art, file], art.id)
    setBinding('copyPath', 'alt+mod+shift+c')
    try {
      await act(async () => {
        window.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', bubbles: true, cancelable: true, altKey: true, shiftKey: true, ...(isMac ? { metaKey: true } : { ctrlKey: true }) }))
      })
      // 行为变更（#32 修复，2026-10-25）：产物页签复制盘上路径 .hexagon/<登记 path>；原断言少了 .hexagon/（指到不存在的路径）
      expect(writeText).toHaveBeenLastCalledWith('/Users/me/todo/.hexagon/docs/spec.md')
    } finally {
      resetBinding('copyPath')
    }
  })

  it('Shift+F10 / Menu 键在焦点页签上打开菜单；Esc 关闭后焦点回到该页签', async () => {
    await openTabs([art, file], art.id)
    act(() => tabButton(art.id).focus())
    await keyOn(tabButton(art.id), { key: 'F10', shiftKey: true })
    expect(menu()).not.toBeNull()
    await keyOn(menu()!, { key: 'Escape' })
    expect(menu()).toBeNull()
    // radix FocusScope 在卸载后的下一个宏任务里还焦点
    await act(async () => { await new Promise((r) => setTimeout(r, 0)) })
    expect(document.activeElement).toBe(tabButton(art.id))
    await keyOn(tabButton(art.id), { key: 'ContextMenu' })
    expect(menu()).not.toBeNull()
  })

  it('右键前焦点在别处（输入条）：关闭后焦点回到右键的页签，不回原处', async () => {
    await openTabs([art, file], art.id)
    const input = document.getElementById('composer-input') as HTMLElement
    act(() => input.focus())
    await rightClick(tabEl(file.id))
    await keyOn(menu()!, { key: 'Escape' })
    await act(async () => { await new Promise((r) => setTimeout(r, 0)) })
    expect(document.activeElement).toBe(tabButton(file.id))
    expect(useUiStore.getState().activeTab).toBe(art.id)
  })

  it.runIf(isMac)('macOS ctrl+点击不切页签（系统把它当右键）', async () => {
    await openTabs([art, file], art.id)
    await act(async () => { tabButton(file.id).dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true })) })
    expect(useUiStore.getState().activeTab).toBe(art.id)
  })
})

describe('新动作进命令面板（A16b「定位」）', () => {
  const palette = async () => {
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', bubbles: true, cancelable: true, ...(isMac ? { metaKey: true } : { ctrlKey: true }) }))
    })
    return [...document.querySelectorAll('[data-command-palette] [cmdk-item]')].map((n) => n.textContent ?? '')
  }

  it('当前页签为文件时面板有关闭其他 / 关闭右侧 / 复制路径', async () => {
    await openTabs([art, file], art.id)
    const items = await palette()
    expect(items.some((x) => x.startsWith('Close other tabs'))).toBe(true)
    expect(items.some((x) => x.startsWith('Close tabs to the right'))).toBe(true)
    expect(items.some((x) => x.startsWith('Copy path'))).toBe(true)
  })

  it('当前页签为 Agent 时面板不出复制路径', async () => {
    await openTabs([art, agent], agent.id)
    const items = await palette()
    expect(items.some((x) => x.startsWith('Copy path'))).toBe(false)
  })
})
