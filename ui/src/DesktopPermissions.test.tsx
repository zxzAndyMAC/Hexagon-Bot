import { assertModalBlocksIntake } from './modalDecisionTestHelpers'
import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { DesktopPermissions } from './components/DesktopPermissions'
import { api } from './api'
import i18n from './i18n'
import { handlePendingKey } from './decisions'
import { useUiStore } from './store'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
let root: ReturnType<typeof createRoot>
const el = document.createElement('div')
afterEach(() => { root?.unmount(); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })
it('offers the missing system pane, rechecks on return, and never treats opening Settings as a grant', async () => {
  await i18n.changeLanguage('zh-CN')
  const status = vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'HexagonLive.app', supported: true, available: true, accessibility: false, screen_recording: true, input_events: false, ready: false, error: null })
  const open = vi.spyOn(api, 'desktopOpenSettings').mockResolvedValue()
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => { root.render(<DesktopPermissions />) })
  await act(async () => { el.querySelector('button')!.click() })
  expect(el.querySelector('[role=dialog]')).not.toBeNull()
  const settings = [...el.querySelectorAll('button')].find(b => b.textContent?.includes('前往辅助功能'))!
  await act(async () => { settings.click() })
  expect(open).toHaveBeenCalledWith('accessibility')
  expect(el.textContent).toContain('尚未授权')
  status.mockResolvedValue({ host_name: 'HexagonLive.app', supported: true, available: true, accessibility: true, screen_recording: true, input_events: true, ready: true, error: null })
  await act(async () => { window.dispatchEvent(new Event('focus')) })
  expect(el.textContent).toContain('系统权限已就绪')
})
it('does not confuse a broken execution component with denied permissions', async () => {
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'HexagonLive.app', supported: true, available: false, accessibility: false, screen_recording: false, input_events: false, ready: false, error: 'loader failed' })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => { root.render(<DesktopPermissions />) })
  await act(async () => { el.querySelector('button')!.click() })
  expect(el.textContent).toContain('执行组件无法启动')
  expect(el.textContent).not.toContain('前往辅助功能')
})

it('blocks hidden decisions while the system modal is open, preserving Settings keys and newer scopes', async () => {
  useUiStore.setState({ modalScope: 'workbench', toasts: [] })
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'HexagonLive', supported: true, available: true, accessibility: false, screen_recording: true, input_events: false, ready: false, error: null })
  const settings = vi.spyOn(api, 'desktopOpenSettings').mockResolvedValue()
  const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue()
  const pending = (event: KeyboardEvent) => { void handlePendingKey(event, [{ id: 'q1', kind: 'permission', agent_id: null, payload: {}, state: 'queued' }]) }
  const behind = vi.fn()
  window.addEventListener('keydown', pending)
  window.addEventListener('keydown', behind)
  try {
    root = createRoot(el); document.body.appendChild(el)
    await act(async () => root.render(<DesktopPermissions />))
    await act(async () => el.querySelector('button')!.click())
    for (const key of ['Enter', 'Backspace']) {
      await act(async () => el.querySelector('dialog')!.dispatchEvent(new KeyboardEvent('keydown', { key, ctrlKey: true, bubbles: true, cancelable: true })))
    }
    for (const [key, shiftKey] of [['i', false], ['Backspace', false], ['Enter', false], ['Enter', true]] as const) {
      await act(async () => el.querySelector('dialog')!.dispatchEvent(new KeyboardEvent('keydown', { key, ctrlKey: true, altKey: true, shiftKey, bubbles: true, cancelable: true })))
    }
    await assertModalBlocksIntake()
    expect(answer).not.toHaveBeenCalled()
    expect(behind).not.toHaveBeenCalled()
    await act(async () => el.querySelector('dialog')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, altKey: true, shiftKey: true, bubbles: true })))
    expect(settings).toHaveBeenCalledWith('accessibility')
    useUiStore.setState({ modalScope: 'palette' })
    // Native Escape emits cancel; closing must not restore an obsolete scope.
    await act(async () => el.querySelector('dialog')!.dispatchEvent(new Event('cancel', { cancelable: true })))
    expect(el.querySelector('dialog')).toBeNull()
    expect(useUiStore.getState().modalScope).toBe('palette')
    useUiStore.setState({ modalScope: 'workbench' })
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, cancelable: true })))
    expect(answer).toHaveBeenCalledOnce()
  } finally { window.removeEventListener('keydown', pending); window.removeEventListener('keydown', behind); useUiStore.setState({ modalScope: 'workbench' }) }
})
