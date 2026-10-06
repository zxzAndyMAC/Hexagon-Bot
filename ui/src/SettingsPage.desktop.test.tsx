import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { SettingsPage } from './components/SettingsPage'
import { api } from './api'
import i18n from './i18n'
import { DesktopPauseButton } from './components/DesktopPauseButton'
import { useUiStore } from './store'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const status = { project_root: '/project-a', enabled: true, active_project: null, active_agent: null, busy: false, paused: false, outcome_unknown: false, screenshot_count: 0 }
const header = () => el.firstElementChild!.firstElementChild!
const buttons = () => [...header().querySelectorAll('button')].filter(button => ['Pause computer', 'Resume computer', 'Computer access disabled'].includes(button.textContent!.trim()))

beforeEach(async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(status)
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
})
afterEach(async () => { await act(async () => root.unmount()); el.remove(); vi.useRealTimers(); vi.restoreAllMocks() })

it('uses one settings button for pause and resume, reflecting project disable immediately', async () => {
  const read = vi.mocked(api.desktopStatus)
  vi.spyOn(api, 'desktopControl').mockImplementation(async action => {
    const next = { ...status, paused: action === 'pause', enabled: action !== 'disable' }
    read.mockResolvedValue(next)
    return next
  })
  await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="perms" />))
  expect(buttons()).toHaveLength(1)
  expect(buttons()[0].textContent).toContain('Pause computer')
  await act(async () => buttons()[0].click())
  expect(buttons()).toHaveLength(1)
  expect(buttons()[0].textContent).toContain('Resume computer')
  await act(async () => buttons()[0].click())
  expect(buttons()[0].textContent).toContain('Pause computer')
  const disable = [...el.querySelectorAll('button')].find(button => button.textContent!.trim() === 'Disable')!
  await act(async () => disable.click())
  expect(buttons()).toHaveLength(1)
  expect(buttons()[0].textContent).toContain('Computer access disabled')
  expect(buttons()[0].disabled).toBe(true)
})

it('keeps unavailable and occupied resume controls disabled without changing host state', async () => {
  const read = vi.mocked(api.desktopStatus)
  read.mockRejectedValueOnce(new Error('host unavailable'))
  const control = vi.spyOn(api, 'desktopControl')
  await act(async () => root.render(<DesktopPauseButton />))
  expect(el.querySelector('button')!.disabled).toBe(true)
  expect(el.querySelector('button')!.title).toContain('host unavailable')
  for (const occupied of [{ busy: true, active_project: null }, { busy: false, active_project: '/other' }]) {
    read.mockResolvedValue({ ...status, paused: true, ...occupied })
    await act(async () => window.dispatchEvent(new Event('focus')))
    expect(el.querySelector('button')!.textContent).toContain('Resume computer')
    expect(el.querySelector('button')!.disabled).toBe(true)
    await act(async () => el.querySelector('button')!.click())
  }
  expect(control).not.toHaveBeenCalled()
})

it('discards earlier status responses across refreshes and project switches', async () => {
  let old!: (value: typeof status) => void
  vi.mocked(api.desktopStatus).mockImplementationOnce(() => new Promise(resolve => { old = resolve }))
  await act(async () => root.render(<DesktopPauseButton />))
  await act(async () => window.dispatchEvent(new Event('focus')))
  expect(el.querySelector('button')!.textContent).toContain('Pause computer')
  await act(async () => old({ ...status, paused: true }))
  expect(el.querySelector('button')!.textContent).toContain('Pause computer')
  let switched!: (value: typeof status) => void
  vi.mocked(api.desktopStatus).mockImplementationOnce(() => new Promise(resolve => { switched = resolve }))
  const epoch = useUiStore.getState().projectEpoch
  await act(async () => useUiStore.setState({ projectEpoch: epoch + 1 }))
  expect(el.querySelector('button')!.disabled).toBe(true)
  await act(async () => switched({ ...status, project_root: '/project-b', enabled: false }))
  expect(el.querySelector('button')!.textContent).toContain('Computer access disabled')
  await act(async () => useUiStore.setState({ projectEpoch: epoch }))
})
