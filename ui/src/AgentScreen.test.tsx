import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { AgentScreen } from './components/AgentScreen'
import { api } from './api'
import { useUiStore } from './store'
import i18n from './i18n'

vi.mock('./api', () => ({
  errText: (e: unknown) => String(e),
  api: {
    desktopStatus: vi.fn(), browserStatus: vi.fn(), desktopPreviewTarget: vi.fn(),
    browserPreview: vi.fn(), desktopPreview: vi.fn(), desktopPreviewStop: vi.fn(),
    browserFocus: vi.fn(), desktopPreviewFocus: vi.fn(), desktopControl: vi.fn(),
  },
}))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const status = { project_root: '/a', enabled: true, active_project: '/a', active_agent: 'a1', busy: false, paused: false, outcome_unknown: false, screenshot_count: 0 }
const browser = { project_root: '/a', session_id: 'session1', mode: 'managed' as const, tab_id: 'tab1', navigation_generation: 1, url: 'http://localhost:4173', title: 'Test page', connected: true }
const frame = { session_id: 'session1', tab_id: 'tab1', navigation_generation: 1, data_url: 'data:image/jpeg;base64,new', captured_at: 1 }
beforeEach(async () => {
  vi.useFakeTimers(); vi.clearAllMocks()
  useUiStore.setState({ projectEpoch: 0, projectRoot: null })
  await i18n.changeLanguage('en')
  vi.mocked(api.desktopStatus).mockResolvedValue(status)
  vi.mocked(api.browserStatus).mockResolvedValue(browser)
  vi.mocked(api.desktopPreviewTarget).mockResolvedValue(null)
  vi.mocked(api.browserPreview).mockResolvedValue(frame)
  vi.mocked(api.desktopPreviewStop).mockResolvedValue(undefined)
  vi.mocked(api.browserFocus).mockResolvedValue(undefined)
  el = document.createElement('div'); document.body.append(el); root = createRoot(el)
})
afterEach(async () => { await act(async () => root.unmount()); el.remove(); vi.useRealTimers(); vi.restoreAllMocks() })
// 票 ui-redesign 14（D19、ADR 0088 偏差 9 收口）：隐藏后不再留输入条旁的重开按钮，重开只走头部「屏幕」按钮
// （store.requestAgentScreen，与 agentScreen 键同效）。隐藏选择按项目跨重挂保留，所以先前用例隐藏过的要重新打开。
const reopen = () => act(async () => useUiStore.getState().requestAgentScreen())
const render = async () => {
  await act(async () => root.render(<AgentScreen />))
  if (!el.querySelector('[data-agent-screen]') && useUiStore.getState().agentScreenAvailable) await reopen()
}
const hide = () => act(async () => el.querySelector<HTMLButtonElement>('[aria-label="Hide preview"]')!.click())

it('displays only local preview, stops capture while hidden and resumes explicitly', async () => {
  await render()
  expect(el.querySelector('img')?.getAttribute('src')).toContain('new')
  expect(api.browserPreview).toHaveBeenCalledTimes(1)
  await hide()
  const count = vi.mocked(api.browserPreview).mock.calls.length
  await act(async () => vi.advanceTimersByTimeAsync(3000))
  expect(api.browserPreview).toHaveBeenCalledTimes(count)
  expect(el.querySelector('img')).toBeNull()
  expect(api.desktopControl).not.toHaveBeenCalled()
  await reopen()
  expect(api.browserPreview).toHaveBeenCalledTimes(count + 1)
})

it('never queues overlapping frames or revives an in-flight frame after hiding', async () => {
  let resolve!: (value: typeof frame) => void
  vi.mocked(api.browserPreview).mockImplementation(() => new Promise(r => { resolve = r }))
  await render()
  await act(async () => vi.advanceTimersByTimeAsync(4000))
  expect(api.browserPreview).toHaveBeenCalledTimes(1)
  await hide()
  await act(async () => resolve(frame))
  expect(el.querySelector('img')).toBeNull()
  await act(async () => vi.advanceTimersByTimeAsync(2000))
  expect(api.browserPreview).toHaveBeenCalledTimes(1)
})

it('rejects a frame for a different navigation and clears pixels when access is revoked', async () => {
  vi.mocked(api.browserPreview).mockResolvedValue({ ...frame, navigation_generation: 2 })
  await render()
  expect(el.querySelector('img')).toBeNull()
  vi.mocked(api.browserPreview).mockResolvedValue(frame)
  await act(async () => vi.advanceTimersByTimeAsync(500))
  expect(el.querySelector('img')).not.toBeNull()
  vi.mocked(api.desktopStatus).mockResolvedValue({ ...status, enabled: false })
  await act(async () => vi.advanceTimersByTimeAsync(1000))
  expect(el.querySelector('img')).toBeNull()
})

// 票 ui-redesign 14（D19）：浮窗底栏的暂停 / 恢复撤掉——暂停在头部控制状态，恢复在电脑操作面板；
// 浮窗只留「打开真实应用」并用 chip 显示「已暂停」。暂停期间仍允许本地预览（语义不变）。
it('binds app focus to the preview session and permits preview during owner pause', async () => {
  vi.mocked(api.desktopStatus).mockResolvedValue({ ...status, paused: true })
  await render()
  expect(el.querySelector('img')).not.toBeNull()
  expect(el.textContent).toContain('Paused')
  await act(async () => el.querySelector<HTMLButtonElement>('[data-screen-open]')!.click())
  expect(api.browserFocus).toHaveBeenCalledWith('/a', 'session1')
  expect(api.desktopControl).not.toHaveBeenCalled()
})

it('ignores pending pixels from a prior project after the status switches', async () => {
  let resolve!: (value: typeof frame) => void
  vi.mocked(api.browserPreview).mockImplementationOnce(() => new Promise(r => { resolve = r }))
  await render()
  vi.mocked(api.desktopStatus).mockResolvedValue({ ...status, project_root: '/b', enabled: false })
  await act(async () => vi.advanceTimersByTimeAsync(1000))
  await act(async () => resolve(frame))
  expect(el.querySelector('img')).toBeNull()
})

it('keeps Hide across a workbench remount without changing computer permission', async () => {
  await render(); await hide()
  await act(async () => { root.unmount(); root = createRoot(el); root.render(<AgentScreen />) })
  const count = vi.mocked(api.browserPreview).mock.calls.length
  await act(async () => vi.advanceTimersByTimeAsync(2000))
  expect(api.browserPreview).toHaveBeenCalledTimes(count)
  expect(el.querySelector('img')).toBeNull()
  expect(api.desktopControl).not.toHaveBeenCalled()
})

it('discards cached pixels on document hiding and waits for a new frame after return', async () => {
  const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  await render()
  expect(el.querySelector('img')).not.toBeNull()
  hidden.mockReturnValue(true)
  await act(async () => document.dispatchEvent(new Event('visibilitychange')))
  expect(el.querySelector('img')).toBeNull()
  let resolve!: (value: typeof frame) => void
  vi.mocked(api.browserPreview).mockImplementation(() => new Promise(r => { resolve = r }))
  hidden.mockReturnValue(false)
  await act(async () => document.dispatchEvent(new Event('visibilitychange')))
  expect(el.querySelector('img')).toBeNull()
  await act(async () => resolve(frame))
  expect(el.querySelector('img')).not.toBeNull()
})

it('binds native pixels to the live receipt and stops its capture on Hide', async () => {
  vi.mocked(api.browserStatus).mockResolvedValue(null)
  vi.mocked(api.desktopPreviewTarget).mockResolvedValue({ project_root: '/a', window_id: 12, process_id: 34, snapshot_id: 'native-new', bundle_id: 'test.app', title: 'Native window' })
  vi.mocked(api.desktopPreview).mockResolvedValue({ project_root: '/a', window_id: 12, process_id: 34, snapshot_id: 'native-old', data_url: 'data:image/jpeg;base64,old', width: 800, height: 600 })
  await render()
  expect(el.querySelector('img')).toBeNull()
  vi.mocked(api.desktopPreview).mockResolvedValue({ project_root: '/a', window_id: 12, process_id: 34, snapshot_id: 'native-new', data_url: 'data:image/jpeg;base64,new', width: 800, height: 600 })
  await act(async () => vi.advanceTimersByTimeAsync(500))
  expect(el.querySelector('img')).not.toBeNull()
  await hide()
  expect(api.desktopPreviewStop).toHaveBeenCalledWith('/a')
  expect(api.desktopControl).not.toHaveBeenCalled()
})

// Issue14 P2: invalidate at the unified project API boundary, before a status
// poll can report the new root. Cached and in-flight old pixels must both vanish.
it('clears cached pixels and focus immediately at beginProjectSwitch before host status catches up', async () => {
  await render()
  expect(el.querySelector('img')?.getAttribute('src')).toContain('new')
  let resolveOld!: (value: typeof frame) => void
  vi.mocked(api.browserPreview).mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve }))
  await act(async () => vi.advanceTimersByTimeAsync(500))
  const before = vi.mocked(api.desktopStatus).mock.calls.length
  await act(async () => useUiStore.getState().beginProjectSwitch())
  expect(api.desktopStatus).toHaveBeenCalledTimes(before)
  expect(el.querySelector('img')).toBeNull()
  expect(el.querySelector('[data-screen-open]')).toBeNull()
  expect(api.browserFocus).not.toHaveBeenCalled()
  await act(async () => resolveOld(frame))
  expect(el.querySelector('img')).toBeNull()
  vi.mocked(api.desktopStatus).mockResolvedValue({ ...status, project_root: '/b' })
  vi.mocked(api.browserStatus).mockResolvedValue({ ...browser, project_root: '/b', session_id: 'session-b' })
  vi.mocked(api.browserPreview).mockResolvedValue({ ...frame, session_id: 'session-b', data_url: 'data:image/jpeg;base64/B' })
  await act(async () => useUiStore.setState({ projectRoot: '/b' }))
  expect(el.querySelector('img')?.getAttribute('src')).toBe('data:image/jpeg;base64/B')
})

it('rejects an old metadata response after beginProjectSwitch even before replacement root is known', async () => {
  let resolveOld!: (value: typeof browser) => void
  vi.mocked(api.browserStatus).mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve }))
  await render()
  await act(async () => useUiStore.getState().beginProjectSwitch())
  await act(async () => resolveOld(browser))
  expect(el.querySelector('img')).toBeNull()
  expect(api.browserPreview).not.toHaveBeenCalled()
  expect(el.querySelector('[data-screen-open]')).toBeNull()
})

// Owner 2026-10-02: local picker polling and preview share one browser worker.
// A scheduling miss must not blink a recent same-target image out indefinitely.
it('keeps recent pixels during browser contention, expires them, and still clears real failures', async () => {
  await render()
  vi.mocked(api.browserPreview).mockRejectedValue({ code: 'internal', message: 'browser busy; local preview yields to role work' })
  await act(async () => vi.advanceTimersByTimeAsync(500))
  expect(el.querySelector('img')?.getAttribute('src')).toContain('new')
  await act(async () => vi.advanceTimersByTimeAsync(2000))
  expect(el.querySelector('img')).toBeNull()
  vi.mocked(api.browserPreview).mockResolvedValue(frame)
  await act(async () => vi.advanceTimersByTimeAsync(500))
  expect(el.querySelector('img')?.getAttribute('src')).toContain('new')
  vi.mocked(api.browserPreview).mockRejectedValue({ code: 'internal', message: 'browser target changed; observe again' })
  await act(async () => vi.advanceTimersByTimeAsync(500))
  expect(el.querySelector('img')).toBeNull()
})

// Owner issue19: absence of a real target must not leave a disabled toolbar.
it('shows no screen control without a target and removes it after disconnect', async () => {
  vi.mocked(api.browserStatus).mockResolvedValue(null)
  await render()
  expect(el.querySelector('button')).toBeNull()
  vi.mocked(api.browserStatus).mockResolvedValue(browser)
  await act(async () => vi.advanceTimersByTimeAsync(1000))
  expect(el.querySelector('button')).not.toBeNull()
  vi.mocked(api.browserStatus).mockResolvedValue(null)
  await act(async () => vi.advanceTimersByTimeAsync(1000))
  expect(el.querySelector('button')).toBeNull()
})

it('does not revive a historical native target after the agent releases control', async () => {
  vi.mocked(api.browserStatus).mockResolvedValue(null)
  vi.mocked(api.desktopPreviewTarget).mockResolvedValue({ project_root:'/a',window_id:7,process_id:8,bundle_id:'test.app',title:'Old window',snapshot_id:'old' })
  vi.mocked(api.desktopStatus).mockResolvedValue({ ...status, active_agent:null, active_project:null })
  await render()
  expect(el.querySelector('button')).toBeNull()
  expect(api.desktopPreview).not.toHaveBeenCalled()
})
