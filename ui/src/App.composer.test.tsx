import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import App from './App'
import { isMac } from './keymap'
import { api } from './api'
import * as apiModule from './api'
import { useUiStore, type WorkTab } from './store'
import i18n from './i18n'
import { mainView } from './layout'

// happy-dom has no canvas renderer; exercise real tabs with the chart SDK stubbed.
vi.mock('echarts/core', () => ({ use: vi.fn(), init: () => ({ setOption: vi.fn(), resize: vi.fn(), dispose: vi.fn() }) }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()

function visible(element: HTMLElement) {
  for (let node: HTMLElement | null = element; node; node = node.parentElement) {
    if (node.hidden || node.style.display === 'none') return false
  }
  return true
}

beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.removeItem('hexagon.layout.canvas')
  useUiStore.setState({ projectRoot: null, projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false,
    canvasLayout: { width: null, full: false }, usageView: false, focusSnapshot: null,
    team: [], pending: [], timeline: [], modalScope: 'workbench', intakeDraft: false })
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
  useUiStore.setState(initial)
})

// ADR 0090（ui-redesign 票 03）：改写 Owner 2026-10-06「非时间线 Tab 隐藏输入条」。
// 时间线不再是页签，输入条跟着对话常驻：画布并排打开任何页签时都在；
// 只在画布全宽或用量详情占满主区时随对话收起，回来后草稿、附件、网页引用原样保留（隐藏不卸载）。
it('keeps the input beside every canvas tab, hides it only for full-width canvas and usage detail, and keeps the draft', async () => {
  const input = el.querySelector<HTMLTextAreaElement>('#composer-input')!
  expect(visible(input)).toBe(true)
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, 'Unsent draft')
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  const canvasTabs: WorkTab[] = [
    { id: 'file:README.md', kind: 'file', title: 'README.md', path: 'README.md' },
    { id: 'art:spec.md', kind: 'artifact', title: 'spec.md', path: 'spec.md' },
    { id: 'agent:a1', kind: 'agent', title: 'Agent', agentId: 'a1' },
    { id: 'diff:spec', kind: 'diff', title: 'Diff', patchText: '' },
  ]
  for (const tab of canvasTabs) {
    await act(async () => useUiStore.getState().openTab(tab))
    expect(visible(input), tab.kind).toBe(true)
  }
  await act(async () => useUiStore.getState().toggleCanvasFull())
  expect(visible(input), 'full-width canvas').toBe(false)
  await act(async () => useUiStore.getState().showConversation())
  expect(visible(input)).toBe(true)
  await act(async () => useUiStore.getState().openUsageDetail())
  expect(visible(input), 'usage detail').toBe(false)
  await act(async () => useUiStore.getState().showConversation())
  const restored = el.querySelector<HTMLTextAreaElement>('#composer-input')!
  expect(restored).toBe(input)
  expect(visible(restored)).toBe(true)
  expect(restored.value).toBe('Unsent draft')
})

// ADR 0090：⌘N 先退出画布全宽 / 用量详情，再聚焦输入条（interaction §3 §12）。
it.each([
  ['full-width canvas', () => { useUiStore.getState().openTab({ id: 'diff:spec', kind: 'diff', title: 'Diff', patchText: '' }); useUiStore.getState().toggleCanvasFull() }],
  ['usage detail', () => useUiStore.getState().openUsageDetail()],
])('returns to the conversation when the focus-input shortcut is used from %s', async (_name, enter) => {
  await act(async () => enter())
  await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, metaKey: false, bubbles: true, cancelable: true })))
  await act(async () => { await new Promise<void>(resolve => requestAnimationFrame(() => resolve())) })
  expect(mainView(useUiStore.getState())).toBe('chat')
  expect(document.activeElement).toBe(el.querySelector('#composer-input'))
})

// Benchmark I1: visiting Settings must not silently discard an editor draft
// before the project-leave guard can see it.
it('keeps unsaved file text and its leave guard while visiting settings', async () => {
  await act(async () => {
    useUiStore.setState({projectRoot:'/test'})
    useUiStore.getState().openTab({id:'file:README.md',kind:'file',title:'README.md',path:'README.md'})
  })
  const editor=el.querySelector<HTMLTextAreaElement>('textarea[aria-label="README.md"]')!
  expect(editor).toBeTruthy()
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(editor,'unsaved owner text')
    editor.dispatchEvent(new Event('input',{bubbles:true}))
  })
  // 票 09（components D17，interaction §6「离开未保存」）：进设置也过离开守卫。原断言「带着脏稿进设置、回来稿还在」
  // 改为「进设置先弹三选一；取消则留在工作台、稿还在」——工作台在设置下仍不卸载（Benchmark I1），守卫只是多一道确认。
  await act(async () => window.dispatchEvent(new KeyboardEvent('keydown',{key:',',ctrlKey:!isMac,metaKey:isMac,bubbles:true,cancelable:true})))
  expect(useUiStore.getState().confirmReq?.variant).toBe('leave')
  expect(useUiStore.getState().modalScope).toBe('workbench')
  await act(async () => useUiStore.getState().clearConfirm())
  expect(useUiStore.getState().modalScope).toBe('workbench')
  expect(Object.values(useUiStore.getState().fileEdits).some(edit=>edit.dirty)).toBe(true)
  const restored=el.querySelector<HTMLTextAreaElement>('textarea[aria-label="README.md"]')!
  expect(restored.value).toBe('unsaved owner text')
})

// Benchmark I1: projectOpen stays true when A is replaced by B. Intake waits
// for a host token; old asynchronously installed listeners must still detach.
it('rebinds intake and streams on host generation changes and cleans late subscriptions', async () => {
  expect(api.runOpeningIntake).not.toHaveBeenCalled()
  let finishOld!: (stop: () => void) => void
  const stopOld = vi.fn(), stopNew = vi.fn()
  const turns = vi.spyOn(apiModule, 'onTurnDelta')
    .mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve }))
    .mockResolvedValue(stopNew)
  const tools = vi.spyOn(apiModule, 'onToolOutput').mockResolvedValue(() => {})
  await act(async () => useUiStore.setState({ projectRoot: '/repo/a', projectGeneration: 31 }))
  expect(api.runOpeningIntake).toHaveBeenCalledTimes(1)
  expect(turns).toHaveBeenCalledTimes(1)
  await act(async () => useUiStore.setState({ projectGeneration: 32 }))
  expect(api.runOpeningIntake).toHaveBeenCalledTimes(2)
  expect(turns).toHaveBeenCalledTimes(2)
  expect(tools).toHaveBeenCalledTimes(2)
  await act(async () => finishOld(stopOld))
  expect(stopOld).toHaveBeenCalledTimes(1)
  expect(stopNew).not.toHaveBeenCalled()
  await act(async () => useUiStore.getState().beginProjectSwitch())
  expect(stopNew).toHaveBeenCalledTimes(1)
  expect(api.runOpeningIntake).toHaveBeenCalledTimes(2)
})
