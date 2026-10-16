// App 级画布内容（ui-redesign 票 09；components D13–D17、B7，interaction §3「画布联动」§6）。
// ① 选区浮条「改写」只把选区作为引用填进输入条、不发送；全宽时先回到并排（输入条在全宽时随对话收起）。
// ② 编辑 / 预览 / 对比切换不卸载编辑器（同一个编辑面元素、缓冲保留）。
// ③ 离开守卫：进设置时有脏稿先三选一（关页签、切项目见 FileEditor.test）。
// ④ 全宽画布页签栏露出电脑控制状态与托管胶囊（协调裁决：票 03 偏差 4），头部不重复挂。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { ConfirmHost } from './components/ConfirmDialog'
import { api } from './api'
import { useUiStore } from './store'
import { isMac } from './keymap'
import i18n from './i18n'

// 票 04（ADR 0089）：待决弹窗 PendingDialog 换成待决坞 PendingDock，桩随之改名
vi.mock('./components/PendingDock', () => ({ PendingDock: () => null }))
vi.mock('./components/SettingsPage', () => ({ SettingsPage: () => <div data-settings-page /> }))
vi.mock('echarts/core', () => ({ use: vi.fn(), init: () => ({ setOption: vi.fn(), resize: vi.fn(), dispose: vi.fn() }) }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()
const mod = isMac ? { metaKey: true } : { ctrlKey: true }
const key = (k: string, mods: KeyboardEventInit = {}) => act(async () => {
  window.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...mods }))
})
const canvas = () => el.querySelector('[data-canvas]') as HTMLElement
const conversation = () => el.querySelector('[data-conversation]') as HTMLElement
const editor = () => el.querySelector('[data-testid="file-editor"] textarea') as HTMLTextAreaElement
const seg = (label: string) => [...el.querySelectorAll<HTMLButtonElement>('[data-slot="segmented-item"]')].find((b) => b.textContent === label)!

async function type(ta: HTMLTextAreaElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(ta, value)
    ta.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

async function openFile(path = 'README.md') {
  await act(async () => useUiStore.getState().openTab({ id: `file:${path}`, kind: 'file', title: path, path }))
  await act(async () => {})
}

beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.removeItem('hexagon.layout.canvas')
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1440 })
  useUiStore.setState({
    projectRoot: '/test', projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null,
    canvasLayout: { width: null, full: false }, team: [], pending: [], artifacts: [], timeline: [], modalScope: 'workbench',
    intakeDraft: false, sideLayout: { width: 252, collapsed: false }, usageTotal: null, confirmReq: null, fileEdits: {},
    sessionSummary: null, sessionError: null,
  })
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(api, 'listRepoDir').mockResolvedValue([])
  vi.spyOn(api, 'readRepoFileSnapshot').mockImplementation(async (path, project_root) => ({ path, project_root, generation: 1, content: 'alpha\nbeta\ngamma\n' }))
  vi.spyOn(api, 'repoFileHead').mockResolvedValue(null)
  vi.spyOn(api, 'readRepoFile').mockResolvedValue('alpha\nbeta\ngamma\n')
  vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  // 确认层宿主挂在 main.tsx，这里一起挂上
  await act(async () => root.render(<><App /><ConfirmHost /></>))
})

afterEach(async () => {
  await act(async () => root.unmount())
  el.remove()
  document.body.replaceChildren()
  vi.restoreAllMocks()
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState(initial, true)
})

describe('选区浮条（D14 selbar，interaction §3「画布联动」）', () => {
  it('「改写」只把选区作为引用填进输入条，不发送；全宽时先回到并排', async () => {
    const send = vi.spyOn(api, 'sendMessage').mockResolvedValue(undefined as never)
    await openFile()
    await key('\\', mod)
    expect(canvas().dataset.canvas).toBe('full')
    expect(conversation().hidden).toBe(true)

    const ta = editor()
    await act(async () => {
      ta.setSelectionRange(6, 10) // "beta"
      ta.dispatchEvent(new Event('select', { bubbles: true }))
    })
    const rewrite = el.querySelector('[data-sel-rewrite]') as HTMLButtonElement
    expect(rewrite).toBeTruthy()
    await act(async () => { rewrite.click() })

    expect(canvas().dataset.canvas).toBe('side')
    expect(conversation().hidden).toBe(false)
    const composer = document.getElementById('composer-input') as HTMLTextAreaElement
    expect(composer.value).toBe('Please rewrite this part of README.md:2:\n> beta')
    expect(send).not.toHaveBeenCalled()
    expect(el.querySelector('[data-selbar]')).toBeNull()
  })

  it('「描述修改」带上负责人写的描述，同样只填不发', async () => {
    const send = vi.spyOn(api, 'sendMessage').mockResolvedValue(undefined as never)
    await openFile()
    const ta = editor()
    await act(async () => {
      ta.setSelectionRange(0, 5)
      ta.dispatchEvent(new Event('select', { bubbles: true }))
    })
    const input = el.querySelector('[data-selbar] input') as HTMLInputElement
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'Make it uppercase')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => { (el.querySelector('[data-sel-send]') as HTMLButtonElement).click() })
    expect((document.getElementById('composer-input') as HTMLTextAreaElement).value).toBe('Make it uppercase (README.md:1):\n> alpha')
    expect(send).not.toHaveBeenCalled()
  })
})

describe('文件编辑器（D14）', () => {
  it('编辑 / 预览 / 对比切换不卸载编辑器，缓冲保留', async () => {
    await openFile('notes.md')
    const ta = editor()
    await type(ta, 'alpha\nbeta\ndelta\n')
    await act(async () => { seg('Preview').click() })
    expect(el.querySelector('[data-file-preview]')!.textContent).toContain('delta')
    await act(async () => { seg('Compare').click() })
    await act(async () => {})
    expect(el.querySelector('[data-diff="ins"]')!.textContent).toContain('delta')
    expect(el.querySelector('[data-diff="del"]')!.textContent).toContain('gamma')
    await act(async () => { seg('Edit').click() })
    // 同一个元素：没有卸载重挂（撤销栈与滚动位才保得住）
    expect(editor()).toBe(ta)
    expect(ta.value).toBe('alpha\nbeta\ndelta\n')
  })
})

describe('离开未保存（D17）', () => {
  it('进设置时有脏稿先三选一；取消留在工作台，放弃后进设置且脏标记清除', async () => {
    await openFile()
    await type(editor(), 'changed')
    await key(',', mod)
    const dialog = document.querySelector('[data-leave-guard]')!
    expect(dialog.textContent).toContain('README.md has unsaved changes')
    // 票 16a 回归（canvas#leave）：去掉 A17 外壳的 gap-3 / p-5，间距只由子项 mt 给（原型 padding 20 20 14、标题→说明 4、说明→按钮 20）；
    // 原先外壳 gap 叠在 mt 上，三段间距各多 12、底边多 6。
    expect(dialog.className).toMatch(/\bgap-0\b/)
    expect(dialog.className).toMatch(/\bpb-3\.5\b/)
    expect(el.querySelector('[data-settings-page]')).toBeNull()
    await act(async () => { (document.querySelector('[data-leave="cancel"]') as HTMLButtonElement).click() })
    expect(document.querySelector('[data-leave-guard]')).toBeNull()
    expect(el.querySelector('[data-settings-page]')).toBeNull()
    expect(editor().value).toBe('changed')

    await key(',', mod)
    await act(async () => { (document.querySelector('[data-leave="discard"]') as HTMLButtonElement).click() })
    await act(async () => {})
    expect(el.querySelector('[data-settings-page]')).toBeTruthy()
    expect(editor().value).toBe('alpha\nbeta\ngamma\n')
    expect(Object.values(useUiStore.getState().fileEdits).some((e) => e.dirty)).toBe(false)
  })

  it('没有脏稿时直接进设置', async () => {
    await openFile()
    await key(',', mod)
    expect(document.querySelector('[data-leave-guard]')).toBeNull()
    expect(el.querySelector('[data-settings-page]')).toBeTruthy()
  })
})

describe('全宽画布露出控制状态与托管胶囊（B7 补设计，票 03 偏差 4）', () => {
  it('全宽时页签栏挂电脑控制状态与托管胶囊，头部不重复挂；回到并排后回到头部', async () => {
    const status = { project_root: '/test', enabled: true, active_project: '/test', active_agent: 'a1', busy: false, paused: false, outcome_unknown: false, screenshot_count: 0, capture_preparation: 'ready' as const }
    useUiStore.setState({ team: [{ id: 'a1', role: 'QA', model_slot: 'chat', status: 'active', avatar_hash: null, model: null, duty: null, superior: null }] })
    vi.spyOn(api, 'desktopStatus').mockResolvedValue(status)
    vi.spyOn(api, 'browserStatus').mockResolvedValue(null as never)
    vi.spyOn(api, 'projectDelegation').mockResolvedValue({ enabled: true, paused: false, generation: 1, delegation_id: 'd1' } as never)
    vi.spyOn(api, 'managedProcessing').mockResolvedValue([])
    await act(async () => useUiStore.getState().openTab({ id: 'art:a.md', kind: 'artifact', title: 'a.md', path: 'a.md' }))
    await key('\\', mod)
    await act(async () => { await new Promise((r) => setTimeout(r, 10)) })
    const bar = el.querySelector('[data-canvas-status]')!
    expect(bar.querySelector('[data-control-chip]')).toBeTruthy()
    expect(bar.querySelector('[data-managed-state]')).toBeTruthy()
    expect(el.querySelectorAll('[data-control-chip]')).toHaveLength(1)
    expect(el.querySelectorAll('[data-managed-state]')).toHaveLength(1)

    await act(async () => { (el.querySelector('[data-canvas-conversation]') as HTMLButtonElement).click() })
    await act(async () => { await new Promise((r) => setTimeout(r, 10)) })
    expect(el.querySelector('[data-canvas-status]')).toBeNull()
    expect(el.querySelector('[data-header] [data-control-chip]')).toBeTruthy()
    expect(el.querySelector('[data-header] [data-managed-state]')).toBeTruthy()
  })
})
