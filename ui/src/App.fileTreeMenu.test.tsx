// 文件树行右键菜单（components A16b 第一批「文件树行」，owner-followups-1015 #07）。
// 菜单项与顺序（种类不适用不渲染）、右键不打开不选中、Shift+F10 打开、插入到输入条（无障碍 §3 拖拽替代）、
// 复制路径、在访达中显示（越界走 errors.reveal_outside_project toast）、平台文案、键位入口、命令面板。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore } from './store'
import { isMac, resetBinding, setBinding } from './keymap'
import { revealLabelKey } from './fileActions'
import i18n from './i18n'
import type { RepoEntry } from './gen/RepoEntry'

vi.mock('./components/PendingDock', () => ({ PendingDock: () => null }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const TREE: Record<string, RepoEntry[]> = {
  '': [
    { name: 'src', path: 'src', kind: 'dir' },
    { name: 'README.md', path: 'README.md', kind: 'file' },
    { name: 'vendor', path: 'vendor', kind: 'link' },
  ],
  src: [{ name: 'main.ts', path: 'src/main.ts', kind: 'file' }],
}

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()

const treeitem = (name: string) =>
  [...el.querySelectorAll<HTMLElement>('[role=treeitem]')].find((n) => n.textContent?.includes(name))!
const menu = () => document.querySelector('[data-slot="context-menu-content"]') as HTMLElement | null
const rows = () => [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')]
const item = (id: string) => rows().find((r) => r.dataset.ctxItem === id)!
const rightClick = (target: Element) => act(async () => {
  const r = target.getBoundingClientRect()
  target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: r.left + 4, clientY: r.top + 4 }))
})
const keyOn = (target: Element, init: KeyboardEventInit) => act(async () => {
  target.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }))
})
const pick = (id: string) => act(async () => { item(id).click() })
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })
const composer = () => document.getElementById('composer-input') as HTMLTextAreaElement

beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState({
    projectRoot: '/test', projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null,
    canvasLayout: { width: null, full: false }, team: [], pending: [], artifacts: [], timeline: [], modalScope: 'workbench',
    intakeDraft: false, usageTotal: null, confirmReq: null, sessionSummary: null, sessionError: null, fileEdits: {},
  })
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(api, 'listRepoDir').mockImplementation(async (rel = '') => TREE[rel] ?? [])
  vi.spyOn(api, 'revealRepoPath').mockResolvedValue(undefined)
  vi.spyOn(api, 'openWithCandidates').mockResolvedValue([])
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
  resetBinding('revealPath')
  resetBinding('insertPath')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState(initial)
})

describe('文件树行右键菜单（A16b 第一批）', () => {
  it('文件：打开 · 插入到输入条 · 复制路径 · 在访达中显示，依此顺序', async () => {
    await rightClick(treeitem('README.md'))
    // #31：末尾加「打开方式 ▸」子菜单触发行。
    expect(rows().map((r) => r.dataset.ctxItem)).toEqual(['open', 'insert', 'copyPath', 'reveal', 'openWith'])
    expect(item('reveal').textContent).toBe(i18n.t(revealLabelKey()))
  })

  it('目录没有「打开」；链接只有复制 / 揭示（种类不适用 → 不渲染）', async () => {
    await rightClick(treeitem('src'))
    // #31：目录也有「打开方式」（只列能打开文件夹的应用）；链接没有。
    expect(rows().map((r) => r.dataset.ctxItem)).toEqual(['insert', 'copyPath', 'reveal', 'openWith'])
    await keyOn(menu()!, { key: 'Escape' })
    await rightClick(treeitem('vendor'))
    expect(rows().map((r) => r.dataset.ctxItem)).toEqual(['copyPath', 'reveal'])
  })

  it('右键不打开、不选中：没开页签，只给行出焦点环标记', async () => {
    const before = useUiStore.getState().tabs.map((t) => t.id)
    await rightClick(treeitem('README.md'))
    expect(useUiStore.getState().tabs.map((t) => t.id)).toEqual(before)
    expect(treeitem('README.md').dataset.state).toBe('open')
    await pick('open')
    expect(useUiStore.getState().activeTab).toBe('file:README.md')
  })

  it.runIf(isMac)('macOS ctrl+点击不打开文件（系统把它当右键）', async () => {
    await act(async () => { treeitem('README.md').dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: true })) })
    expect(useUiStore.getState().tabs.some((t) => t.id === 'file:README.md')).toBe(false)
  })

  it('无障碍 §3：Shift+F10 在焦点行打开 → 插入到输入条，插入的是 # 路径块；Esc 后焦点回行', async () => {
    const row = treeitem('README.md')
    act(() => row.focus())
    await keyOn(row, { key: 'F10', shiftKey: true })
    expect(menu()).not.toBeNull()
    await keyOn(menu()!, { key: 'Escape' })
    await flush()
    expect(document.activeElement).toBe(row)
    await keyOn(row, { key: 'ContextMenu' })
    await pick('insert')
    expect(composer().value).toBe('#README.md ')
  })

  it('插入目录补尾 /，追加到已有草稿之后', async () => {
    await rightClick(treeitem('README.md'))
    await pick('insert')
    await rightClick(treeitem('src'))
    await pick('insert')
    expect(composer().value).toBe('#README.md #src/ ')
  })

  // 行为变更（owner-followups-1015 #30 第 3 项，2026-10-20 负责人裁决）：#07 起文件树行写项目相对路径，
  // 票 29 把画布页签改成绝对路径后两处不一致（同一文件右键两处复制出两种形）。文件树行也改写绝对路径，
  // 与页签同走 fileActions.absolutePath。项目根 = 测试里的 '/test'。
  it('复制路径：写入绝对路径并出回执（与画布页签同口径，#30）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    await act(async () => { treeitem('src').click() })
    await flush()
    await rightClick(treeitem('main.ts'))
    await pick('copyPath')
    expect(writeText).toHaveBeenCalledWith('/test/src/main.ts')
    expect(useUiStore.getState().toasts.at(-1)?.text).toBe('Path copied')
  })

  it('复制路径：copyPath 键位在焦点文件树行上同样写绝对路径（#30）', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    setBinding('copyPath', 'alt+mod+shift+c')
    try {
      const row = treeitem('README.md')
      act(() => row.focus())
      await act(async () => {
        row.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', code: 'KeyC', altKey: true, shiftKey: true, bubbles: true, cancelable: true, ...(isMac ? { metaKey: true } : { ctrlKey: true }) }))
      })
      expect(writeText).toHaveBeenLastCalledWith('/test/README.md')
    } finally {
      resetBinding('copyPath')
    }
  })

  it('在访达中显示：交给宿主命令；越界被 core 拒时出 errors.reveal_outside_project', async () => {
    await rightClick(treeitem('README.md'))
    await pick('reveal')
    expect(api.revealRepoPath).toHaveBeenCalledWith('README.md')
    vi.mocked(api.revealRepoPath).mockRejectedValueOnce({ code: 'reveal_outside_project', message: 'path is outside the project: vendor' })
    await rightClick(treeitem('vendor'))
    await pick('reveal')
    await flush()
    expect(useUiStore.getState().toasts.at(-1)?.text).toContain(i18n.t('errors.reveal_outside_project'))
  })

  it('键位入口：绑定后在焦点行上按 → 作用于该行', async () => {
    setBinding('revealPath', 'alt+mod+shift+y')
    const row = treeitem('src')
    act(() => row.focus())
    await act(async () => {
      row.dispatchEvent(new KeyboardEvent('keydown', { key: 'y', code: 'KeyY', altKey: true, shiftKey: true, bubbles: true, cancelable: true, ...(isMac ? { metaKey: true } : { ctrlKey: true }) }))
    })
    expect(api.revealRepoPath).toHaveBeenCalledWith('src')
  })
})

describe('平台文案', () => {
  it('访达 / 资源管理器 / 文件管理器', () => {
    expect(revealLabelKey('mac')).toBe('ctx.revealMac')
    expect(revealLabelKey('windows')).toBe('ctx.revealWindows')
    expect(revealLabelKey('linux')).toBe('ctx.revealLinux')
    expect(i18n.t('ctx.revealMac', { lng: 'zh-CN' })).toBe('在访达中显示')
    expect(i18n.t('ctx.revealWindows', { lng: 'zh-CN' })).toBe('在资源管理器中显示')
  })
})

describe('新动作进命令面板（A16b「定位」）', () => {
  it('当前页签为文件时面板有插入路径 / 在访达中显示', async () => {
    await act(async () => { useUiStore.getState().openTab({ id: 'file:README.md', kind: 'file', title: 'README.md', path: 'README.md' }) })
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', bubbles: true, cancelable: true, ...(isMac ? { metaKey: true } : { ctrlKey: true }) }))
    })
    const items = [...document.querySelectorAll('[data-command-palette] [cmdk-item]')].map((n) => n.textContent ?? '')
    expect(items.some((x) => x.startsWith('Insert path into composer'))).toBe(true)
    expect(items.some((x) => x.startsWith(i18n.t(revealLabelKey())))).toBe(true)
  })
})
