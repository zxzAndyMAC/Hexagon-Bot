// 功能缺口 C4（票 14 留尾）：头部 chip、电脑操作面板、项目浏览器面板、设置暂停按钮、任务前检查卡
// 共用一份电脑操控状态读——同时挂着也只有一个 2 秒定时器、每拍一次 desktop_status。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { api } from './api'
import { useUiStore } from './store'
import { DESKTOP_POLL_MS, publishDesktopStatus } from './useDesktopStatus'
import { ComputerControlStatus } from './components/ComputerControlStatus'
import { DesktopControlPanel } from './components/DesktopControlPanel'
import { BrowserControlPanel } from './components/BrowserControlPanel'
import { DesktopPauseButton } from './components/DesktopPauseButton'
import { ComputerTaskReadiness } from './components/ComputerTaskReadiness'
import i18n from './i18n'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const status = { project_root: '/project-a', enabled: true, active_project: '/project-a', active_agent: 'id-QA', busy: false, paused: false, outcome_unknown: false, screenshot_count: 0, capture_preparation: 'ready' as const }
const initial = useUiStore.getState()
let root: ReturnType<typeof createRoot> | undefined
const el = document.createElement('div')

beforeEach(async () => {
  await i18n.changeLanguage('en')
  vi.useFakeTimers()
  useUiStore.setState({ projectRoot: '/project-a', projectEpoch: 0, team: [] })
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'Hexagon', supported: true, available: true, accessibility: true, input_events: true, screen_recording: true, ready: true, error: null })
  vi.spyOn(api, 'browserStatus').mockResolvedValue(null)
  vi.spyOn(api, 'computerModels').mockResolvedValue({ project_root: '/project-a', roles: [], vision_slots: [] })
  document.body.appendChild(el)
})
afterEach(async () => {
  if (root) await act(async () => root!.unmount())
  root = undefined
  el.remove(); el.replaceChildren()
  vi.useRealTimers()
  vi.restoreAllMocks()
  useUiStore.setState(initial, true)
})

const mountAll = async () => {
  root = createRoot(el)
  await act(async () => root!.render(<>
    <ComputerControlStatus /><DesktopControlPanel /><BrowserControlPanel /><DesktopPauseButton /><ComputerTaskReadiness mentions={[]} />
  </>))
}

it('five mounted consumers share one read per tick', async () => {
  const read = vi.spyOn(api, 'desktopStatus').mockResolvedValue(status)
  await mountAll()
  expect(read).toHaveBeenCalledTimes(1) // 同一 tick 的挂载合并成一次读
  read.mockClear()
  await act(async () => { await vi.advanceTimersByTimeAsync(DESKTOP_POLL_MS) })
  expect(read).toHaveBeenCalledTimes(1)
  await act(async () => { window.dispatchEvent(new Event('focus')) })
  expect(read).toHaveBeenCalledTimes(2)
})

it('a published write wins over a late poll and stops polling when the last consumer leaves', async () => {
  let late!: (value: typeof status) => void
  const read = vi.spyOn(api, 'desktopStatus').mockResolvedValueOnce(status).mockImplementationOnce(() => new Promise((r) => { late = r }))
  await mountAll()
  await act(async () => { await vi.advanceTimersByTimeAsync(DESKTOP_POLL_MS) })
  await act(async () => { publishDesktopStatus({ ...status, paused: true }); window.dispatchEvent(new Event('hexagon:desktop-status-changed')) })
  await act(async () => late(status))
  expect(el.querySelector('[data-control-status]')?.getAttribute('data-control-status')).toBe('paused')
  expect(read).toHaveBeenCalledTimes(2) // 发布后紧随的信号不重读
  await act(async () => root!.unmount()); root = undefined
  read.mockClear()
  await act(async () => { await vi.advanceTimersByTimeAsync(DESKTOP_POLL_MS * 3) })
  expect(read).not.toHaveBeenCalled()
})
