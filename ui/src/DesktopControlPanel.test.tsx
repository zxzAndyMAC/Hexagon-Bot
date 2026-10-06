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
it('opens the native permissions dialog instead of enabling screen transmission when a grant is missing', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(initial)
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'HexagonLive.app', supported: true, available: true, accessibility: true, input_events: true, screen_recording: false, ready: false, error: null })
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...initial, enabled: true })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<DesktopControlPanel />))
  await act(async () => el.querySelector('button')!.click())
  expect(el.textContent).toContain('selected model provider')
  await act(async () => [...el.querySelectorAll('button')].find(b => b.textContent === 'Enable for this project')!.click())
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
  await act(async () => el.querySelector<HTMLButtonElement>('[aria-label="Pause computer"]')!.click())
  expect(control).toHaveBeenCalledWith('pause', '/project-a')
  await act(async () => resolve({ ...initial, enabled: true }))
  expect(el.textContent).toContain('Computer paused')
  expect(el.querySelector('[aria-label="Pause computer"]')).toBeNull()
})

it.each([true, false])('discards a late permission preflight from an unmounted project (ready=%s)', async ready => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValueOnce(initial).mockResolvedValue({ ...initial, project_root: '/project-b' })
  const permissions = { host_name: 'HexagonLive', supported: true, available: true, accessibility: ready, input_events: ready, screen_recording: ready, ready, error: null }
  let resolve!: (value: typeof permissions) => void
  vi.spyOn(api, 'desktopPermissions').mockImplementationOnce(() => new Promise(r => { resolve = r }))
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...initial, enabled: true })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<DesktopControlPanel />))
  await act(async () => el.querySelector('button')!.click())
  await act(async () => [...el.querySelectorAll('button')].find(b => b.textContent === 'Enable for this project')!.click())
  await act(async () => { root.unmount(); root = createRoot(el); root.render(<DesktopControlPanel />) })
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
  expect([...el.querySelectorAll('button')].find(b => b.textContent === 'Resume computer')?.disabled).toBe(true)
  expect(el.textContent).toContain('Preparing screenshots')
})
