import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore } from './store'
import i18n from './i18n'

vi.mock('./components/TopBar', () => ({ TopBar: ({ onSettings }: { onSettings: () => void }) => <button onClick={onSettings}>Open settings</button> }))
vi.mock('./components/Composer', () => ({ Composer: () => null }))
vi.mock('./components/SidePanel', () => ({ SidePanel: () => null }))
vi.mock('./components/Timeline', () => ({ Timeline: () => null }))
vi.mock('./components/IntakeBar', () => ({ IntakeBar: () => null }))
vi.mock('./components/TabBar', () => ({ TabBar: () => null }))
vi.mock('./components/PendingCards', () => ({ PendingDialog: () => null }))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
let root: ReturnType<typeof createRoot>
const el = document.createElement('div')
afterEach(async () => { await act(async () => root?.unmount()); el.remove(); el.replaceChildren(); vi.restoreAllMocks() })
it('keeps the explicit computer pause key and button working on the Settings page', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  const status = { project_root: '/project-a', enabled: true, active_project: '/project-a', active_agent: 'a0', busy: true, paused: false, outcome_unknown: false, screenshot_count: 0 }
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(status)
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...status, paused: true })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<App />))
  await act(async () => [...el.querySelectorAll('button')].find(button => button.textContent === 'Open settings')!.click())
  expect(useUiStore.getState().modalScope).toBe('settings')
  await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, altKey: true, shiftKey: true, cancelable: true })))
  expect(control).toHaveBeenCalledExactlyOnceWith('pause', '/project-a')
  await act(async () => [...el.querySelectorAll('button')].find(button => button.textContent?.includes('Pause computer'))!.click())
  expect(control).toHaveBeenCalledTimes(2)
})
