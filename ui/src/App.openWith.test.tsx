// 「打开方式 ▸」子菜单（components A16b，owner-followups-1015 #31）：文件树行 / 画布页签 / 项目块三处入口、
// 子菜单状态（列表 + 系统图标 / 中性图标、空态禁用行、失败 + 重试）、键盘（→ 展开、← 收起、Esc 关闭）、
// 选中即调宿主命令（越界走 errors.open_with_outside_project toast）、keymap + 命令面板「打开方式」视图。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore } from './store'
import { isMac, resetBinding, setBinding } from './keymap'
import i18n from './i18n'
import type { RepoEntry } from './gen/RepoEntry'
import type { OpenWithApp } from './gen/OpenWithApp'

vi.mock('./components/PendingDock', () => ({ PendingDock: () => null }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const TREE: Record<string, RepoEntry[]> = {
  '': [
    { name: 'src', path: 'src', kind: 'dir' },
    { name: 'README.md', path: 'README.md', kind: 'file' },
    { name: 'vendor', path: 'vendor', kind: 'link' },
  ],
}
const APPS: OpenWithApp[] = [
  { id: 'com.todesktop.230313mzl4w4u92', name: 'Cursor', icon: 'data:image/png;base64,AAAA', is_default: false },
  { id: 'dev.zed.Zed', name: 'Zed', icon: null, is_default: false },
]

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()

const treeitem = (name: string) =>
  [...el.querySelectorAll<HTMLElement>('[role=treeitem]')].find((n) => n.textContent?.includes(name))!
const menu = () => document.querySelector('[data-slot="context-menu-content"]') as HTMLElement | null
const trigger = () => menu()!.querySelector<HTMLElement>('[data-ctx-item="openWith"]')
const sub = () => document.querySelector('[data-open-with]') as HTMLElement | null
const subRows = () => [...sub()!.querySelectorAll<HTMLElement>('[role="menuitem"]')]
const rightClick = (target: Element) => act(async () => {
  const r = target.getBoundingClientRect()
  target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: r.left + 4, clientY: r.top + 4 }))
})
const keyOn = (target: Element, init: KeyboardEventInit) => act(async () => {
  target.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }))
})
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })
/** 键盘展开子菜单：聚焦触发行，按 →。 */
const openSub = async () => {
  const tr = trigger()!
  act(() => tr.focus())
  await keyOn(tr, { key: 'ArrowRight' })
  await flush()
}

beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState({
    projectRoot: '/test', projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null,
    canvasLayout: { width: null, full: false }, team: [], pending: [], artifacts: [], timeline: [], modalScope: 'workbench',
    intakeDraft: false, usageTotal: null, confirmReq: null, sessionSummary: null, sessionError: null, fileEdits: {}, toasts: [],
  })
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(api, 'listRepoDir').mockImplementation(async (rel = '') => TREE[rel] ?? [])
  vi.spyOn(api, 'openWithCandidates').mockResolvedValue(APPS)
  vi.spyOn(api, 'openWith').mockResolvedValue(undefined)
  vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => root.render(<App />))
  await act(async () => { useUiStore.getState().openCanvas('files') })
  await flush()
})

afterEach(async () => {
  await act(async () => root.unmount())
  el.remove()
  vi.restoreAllMocks()
  resetBinding('openWith')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState(initial)
})

describe('「打开方式」子菜单（A16b）', () => {
  it('文件行：右键即预取候选；→ 展开，行 = 系统图标 / 中性图标 + 应用名；选中交给宿主', async () => {
    await rightClick(treeitem('README.md'))
    expect(api.openWithCandidates).toHaveBeenCalledWith('README.md')
    expect(trigger()!.textContent).toContain('Open with')
    await openSub()
    expect(subRows().map((r) => r.textContent)).toEqual(['Cursor', 'Zed'])
    expect(subRows()[0].querySelector('img')?.getAttribute('src')).toBe(APPS[0].icon)
    // 取不到图标 → 中性图标（svg），不按名称套品牌
    expect(subRows()[1].querySelector('img')).toBeNull()
    expect(subRows()[1].querySelector('svg')).not.toBeNull()
    await act(async () => { subRows()[1].click() })
    await flush()
    expect(api.openWith).toHaveBeenCalledWith('README.md', 'dev.zed.Zed')
    expect(menu()).toBeNull()
  })

  it('键盘：← 收起子菜单回到触发行，Esc 关闭整个菜单', async () => {
    await rightClick(treeitem('README.md'))
    await openSub()
    expect(sub()).not.toBeNull()
    await keyOn(subRows()[0], { key: 'ArrowLeft' })
    await flush()
    expect(sub()).toBeNull()
    expect(document.activeElement).toBe(trigger())
    await openSub()
    await keyOn(subRows()[0], { key: 'Escape' })
    await flush()
    expect(menu()).toBeNull()
  })

  it('空列表：一行禁用「未找到可用的编辑器」', async () => {
    vi.mocked(api.openWithCandidates).mockResolvedValue([])
    await act(async () => { await i18n.changeLanguage('zh-CN') })
    await rightClick(treeitem('src'))
    expect(api.openWithCandidates).toHaveBeenCalledWith('src')
    await openSub()
    const row = sub()!.querySelector<HTMLElement>('[data-open-with-empty]')!
    expect(row.textContent).toBe('未找到可用的编辑器')
    expect(row.dataset.disabled).toBe('')
  })

  it('读取失败：--red 文案 + 「重试」，重试重读', async () => {
    vi.mocked(api.openWithCandidates).mockRejectedValueOnce(new Error('boom'))
    await rightClick(treeitem('README.md'))
    await openSub()
    const err = sub()!.querySelector<HTMLElement>('[data-open-with-error]')!
    expect(err.textContent).toBe('Couldn’t list editorsRetry')
    expect(err.className).toContain('text-red-ink')
    await act(async () => { err.click() })
    await flush()
    expect(api.openWithCandidates).toHaveBeenCalledTimes(2)
    expect(subRows().map((r) => r.textContent)).toEqual(['Cursor', 'Zed'])
  })

  it('越界被 core 拒：出 errors.open_with_outside_project toast', async () => {
    vi.mocked(api.openWith).mockRejectedValueOnce({ code: 'open_with_outside_project', message: 'path is outside the project: x' })
    await rightClick(treeitem('README.md'))
    await openSub()
    await act(async () => { subRows()[0].click() })
    await flush()
    expect(useUiStore.getState().toasts.at(-1)?.text).toContain(i18n.t('errors.open_with_outside_project'))
  })

  it('链接行没有「打开方式」', async () => {
    await rightClick(treeitem('vendor'))
    expect(trigger()).toBeNull()
  })

  it('产物页签：复制路径之后有「打开方式」，作用于页签路径', async () => {
    await act(async () => { useUiStore.getState().openTab({ id: 'art:docs/spec.md', kind: 'artifact', title: 'docs/spec.md', path: 'docs/spec.md' }) })
    await flush()
    await rightClick(document.querySelector('[data-canvas-tab="art:docs/spec.md"]')!)
    expect(trigger()).not.toBeNull()
    expect(api.openWithCandidates).toHaveBeenCalledWith('docs/spec.md')
  })

  it('项目块右键：只有「打开方式」，作用于整个项目（空路径）', async () => {
    const block = document.querySelector<HTMLElement>('[data-project-block]')!
    await rightClick(block)
    const items = [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')].map((r) => r.dataset.ctxItem)
    expect(items).toEqual(['openWith'])
    expect(api.openWithCandidates).toHaveBeenCalledWith('')
    await openSub()
    await act(async () => { subRows()[0].click() })
    await flush()
    expect(api.openWith).toHaveBeenCalledWith('', APPS[0].id)
  })
})

describe('键位与命令面板', () => {
  const mod = isMac ? { metaKey: true } : { ctrlKey: true }
  const options = () => [...document.querySelectorAll<HTMLElement>('[data-command-palette] [cmdk-item]')]

  it('openWith 默认空绑定；绑定后在焦点行上按 → 面板「打开方式」视图：该文件 + 整个项目', async () => {
    const row = treeitem('README.md')
    act(() => row.focus())
    setBinding('openWith', 'alt+mod+shift+o')
    await act(async () => {
      row.dispatchEvent(new KeyboardEvent('keydown', { key: 'o', code: 'KeyO', altKey: true, shiftKey: true, bubbles: true, cancelable: true, ...mod }))
    })
    await flush()
    expect(api.openWithCandidates).toHaveBeenCalledWith('README.md')
    expect(api.openWithCandidates).toHaveBeenCalledWith('')
    const labels = options().map((n) => n.textContent ?? '')
    expect(labels).toEqual([
      'Open with CursorREADME.md', 'Open with ZedREADME.md',
      'Open project with CursorWhole project', 'Open project with ZedWhole project',
    ])
    await act(async () => { options()[2].click() })
    await flush()
    expect(api.openWith).toHaveBeenCalledWith('', APPS[0].id)
  })

  it('⌘K 面板有「打开方式…」，执行后切到编辑器视图而不关面板', async () => {
    await act(async () => { useUiStore.getState().openTab({ id: 'file:README.md', kind: 'file', title: 'README.md', path: 'README.md' }) })
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', bubbles: true, cancelable: true, ...mod }))
    })
    const entry = options().find((n) => n.textContent?.startsWith('Open with…'))!
    expect(api.openWithCandidates).not.toHaveBeenCalled()
    await act(async () => { entry.click() })
    await flush()
    expect(document.querySelector('[data-command-palette]')).not.toBeNull()
    expect(api.openWithCandidates).toHaveBeenCalledWith('README.md')
    expect(options().map((n) => n.textContent)).toContain('Open with ZedREADME.md')
  })
})
