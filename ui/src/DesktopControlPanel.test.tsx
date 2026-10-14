import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { DesktopControlPanel } from './components/DesktopControlPanel'
import { api } from './api'
import i18n from './i18n'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const initial = { project_root: '/project-a', enabled: false, active_project: null, active_agent: null, busy: false, paused: false, outcome_unknown: false, screenshot_count: 0 }
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(async () => { await act(async () => root?.unmount()); vi.useRealTimers(); vi.restoreAllMocks(); el.remove(); el.replaceChildren() })
it('shows screenshot preparation before the first task', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue({ ...initial, enabled: true, capture_preparation: 'preparing' })
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'Hexagon', supported: true, available: true, accessibility: true, input_events: true, screen_recording: true, ready: true, error: null })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<DesktopControlPanel inline />))
  expect(el.textContent).toContain('Preparing screenshots')
})
// 票 ui-redesign 14（D19 / control#panels）：面板不再自带开关按钮——它住在头部控制状态的弹层里或设置「权限」分区；
// 项目同意改成勾选行上的「同意」（原「为本项目启用」）。设置放置（inline）负责挂权限对话框的监听。
// 按钮改用 A1 Button：禁用走 aria-disabled（票 01 偏差 3），断言随之改读 aria-disabled。
const agree = () => el.querySelector<HTMLButtonElement>('[data-desktop-action=enable]')!
it('opens the native permissions dialog instead of enabling screen transmission when a grant is missing', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(initial)
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'HexagonLive.app', supported: true, available: true, accessibility: true, input_events: true, screen_recording: false, ready: false, error: null })
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...initial, enabled: true })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<DesktopControlPanel inline />))
  // 勾选行直接显示卡在哪一道门：系统授权缺屏幕录制、项目尚未同意
  expect(el.querySelector('[data-desktop-grants]')?.textContent).toContain('Screen Recording')
  await act(async () => agree().click())
  expect(control).not.toHaveBeenCalled()
  expect(el.querySelector('dialog')?.textContent).toContain('Screen Recording')
  expect(el.querySelector('dialog')?.textContent).toContain('Not granted')
})
it('does not let a late status poll undo a completed pause, and keeps Pause separate from Send', async () => {
  vi.useFakeTimers()
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'HexagonLive.app', supported: true, available: true, accessibility: true, input_events: true, screen_recording: true, ready: true, error: null })
  let resolve!: (v: typeof initial) => void
  vi.spyOn(api, 'desktopStatus').mockResolvedValueOnce({ ...initial, enabled: true }).mockImplementationOnce(() => new Promise(r => { resolve = r }))
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...initial, enabled: true, paused: true })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<DesktopControlPanel />))
  await act(async () => { vi.advanceTimersByTime(2000) })
  await act(async () => el.querySelector<HTMLButtonElement>('[data-desktop-action=pause]')!.click())
  expect(control).toHaveBeenCalledWith('pause', '/project-a')
  await act(async () => resolve({ ...initial, enabled: true }))
  expect(el.textContent).toContain('Paused')
  expect(el.querySelector('[data-desktop-action=pause]')).toBeNull()
  expect(el.querySelector('[data-desktop-action=resume]')).not.toBeNull()
})

it.each([true, false])('discards a late permission preflight from an unmounted project (ready=%s)', async ready => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValueOnce(initial).mockResolvedValue({ ...initial, project_root: '/project-b' })
  const permissions = { host_name: 'HexagonLive', supported: true, available: true, accessibility: ready, input_events: ready, screen_recording: ready, ready, error: null }
  let resolve!: (value: typeof permissions) => void
  vi.spyOn(api, 'desktopPermissions').mockImplementationOnce(() => new Promise(r => { resolve = r }))
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...initial, enabled: true })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<DesktopControlPanel inline />))
  await act(async () => agree().click())
  await act(async () => { root.unmount(); root = createRoot(el); root.render(<DesktopControlPanel inline />) })
  await act(async () => resolve(permissions))
  expect(control).not.toHaveBeenCalled()
  expect(el.querySelector('dialog')).toBeNull()
})

it('rechecks a failed preparation without resuming or replaying the paused action', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue({ ...initial, enabled: true, paused: true, outcome_unknown: true, capture_preparation: 'failed' })
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'Hexagon', supported: true, available: true, accessibility: true, input_events: true, screen_recording: true, ready: true, error: null })
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...initial, enabled: true, paused: true, outcome_unknown: true, capture_preparation: 'preparing' })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<DesktopControlPanel inline />))
  await act(async () => [...el.querySelectorAll('button')].find(b => b.textContent === 'Recheck screenshots')!.click())
  expect(control).toHaveBeenCalledExactlyOnceWith('prepare_capture', '/project-a')
  expect(el.querySelector('[data-desktop-action=resume]')?.getAttribute('aria-disabled')).toBe('true')
  expect(el.textContent).toContain('Preparing screenshots')
})

it('shows system grants and project consent as separate gates, with the current controller', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue({ ...initial, enabled: true, active_project: '/project-a', active_agent: 'a1', busy: false, screenshot_count: 2 })
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'Hexagon-Bot.app', supported: true, available: true, accessibility: false, input_events: true, screen_recording: true, ready: false, error: null })
  const settings = vi.spyOn(api, 'desktopOpenSettings').mockResolvedValue()
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...initial, enabled: true })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<DesktopControlPanel />))
  const grants = el.querySelector('[data-desktop-grants]')!
  expect(grants.querySelectorAll('[data-ok]')).toHaveLength(3) // input events, screen recording, project consent
  await act(async () => [...grants.querySelectorAll('button')].find(b => b.textContent === 'Open System Settings')!.click())
  expect(settings).toHaveBeenCalledWith('accessibility')
  expect(el.querySelector('[data-controller]')?.textContent).toContain('a1 · project-a')
  expect(el.textContent).toContain('Hexagon-Bot.app')
  await act(async () => el.querySelector<HTMLButtonElement>('[data-desktop-action=release]')!.click())
  expect(control).toHaveBeenCalledExactlyOnceWith('release', '/project-a')
})
