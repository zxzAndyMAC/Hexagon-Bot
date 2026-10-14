// App 级电脑操控（ui-redesign 票 14；components D19、B5，control#screen / control#panels）。
// 头部控制状态写清谁在控制什么、「暂停电脑操作」带 desktopPause 当前绑定且点击 / 快捷键都走同一暂停路径；
// 面板在暂停时禁用「打开」并写原因；Agent Screen 只剩头部一个重开入口。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore } from './store'
import { bindingFor, formatBinding } from './keymap'
import i18n from './i18n'

vi.mock('./components/shell/Sidebar', () => ({ Sidebar: ({ onSettings }: { onSettings: () => void }) => <button onClick={onSettings}>Open settings</button> }))
vi.mock('./components/Composer', () => ({ Composer: () => null }))
vi.mock('./components/shell/InterimCanvas', () => ({ InterimCanvas: () => null }))
vi.mock('./components/Timeline', () => ({ Timeline: () => null }))
vi.mock('./components/IntakeBar', () => ({ IntakeBar: () => null }))
vi.mock('./components/TabBar', () => ({ TabBar: () => null }))
vi.mock('./components/PendingCards', () => ({ PendingDialog: () => null }))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
const el = document.createElement('div')
const status = { project_root: '/project-a', enabled: true, active_project: '/project-a', active_agent: 'id-QA', busy: false, paused: false, outcome_unknown: false, screenshot_count: 1, capture_preparation: 'ready' as const }
const browser = { project_root: '/project-a', session_id: 's1', mode: 'managed' as const, tab_id: 't1', navigation_generation: 1, url: 'http://localhost:5173', title: 'Todo', connected: true }
const initial = useUiStore.getState()

beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({ projectRoot: '/project-a', projectEpoch: 0, modalScope: 'workbench', team: [{ id: 'id-QA', role: 'QA', model_slot: 'chat', status: 'active' } as never] })
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  vi.spyOn(api, 'desktopPermissions').mockResolvedValue({ host_name: 'Hexagon-Bot.app', supported: true, available: true, accessibility: true, input_events: true, screen_recording: true, ready: true, error: null })
  vi.spyOn(api, 'browserStatus').mockResolvedValue(browser)
  vi.spyOn(api, 'desktopPreviewTarget').mockResolvedValue(null)
  vi.spyOn(api, 'browserPreview').mockResolvedValue({ session_id: 's1', tab_id: 't1', navigation_generation: 1, data_url: 'data:image/jpeg;base64,x', captured_at: 1 })
  document.body.appendChild(el)
})
afterEach(async () => {
  await act(async () => root?.unmount())
  el.remove(); el.replaceChildren(); document.body.replaceChildren()
  vi.restoreAllMocks()
  useUiStore.setState(initial, true)
})
const mount = async () => { root = createRoot(el); await act(async () => root.render(<App />)) }
const header = () => el.querySelector('[data-header]')!

it('names who controls what and pauses from the header button or the global key', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(status)
  const control = vi.spyOn(api, 'desktopControl').mockResolvedValue({ ...status, paused: true })
  await mount()
  expect(header().querySelector('[data-control-chip]')?.textContent).toBe('QA is controlling the browser')
  const pause = header().querySelector<HTMLButtonElement>('[data-header-pause]')!
  expect(pause.textContent).toContain('Pause computer use')
  // R-05：kbd 照 keymap 渲染当前绑定，不写原型的 ⌃⌥⌘.
  expect(pause.querySelector('kbd')?.textContent).toBe(formatBinding(bindingFor('desktopPause')))
  await act(async () => pause.click())
  expect(control).toHaveBeenCalledExactlyOnceWith('pause', '/project-a')
  await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, altKey: true, shiftKey: true, metaKey: false, cancelable: true })))
  // 全局键与按钮走同一路径（desktopPause.pauseDesktop）；jsdom 下 mod=ctrl
  expect(control).toHaveBeenCalledTimes(2)
  expect(control).toHaveBeenLastCalledWith('pause', '/project-a')
})

it('hides the control status when computer use is off and nothing is connected', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue({ ...status, enabled: false, active_agent: null, active_project: null })
  vi.spyOn(api, 'browserStatus').mockResolvedValue(null)
  await mount()
  expect(header().querySelector('[data-control-status]')).toBeNull()
  expect(header().querySelector('[data-header-pause]')).toBeNull()
})

it('opens the panels from the chip; while paused, Open is disabled with the reason and pause gives way', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue({ ...status, paused: true })
  vi.spyOn(api, 'browserStatus').mockResolvedValue(null)
  const open = vi.spyOn(api, 'browserOpen')
  await mount()
  expect(header().querySelector('[data-control-chip]')?.textContent).toBe('Computer use paused')
  expect(header().querySelector('[data-header-pause]')).toBeNull()
  await act(async () => header().querySelector<HTMLButtonElement>('[data-control-chip]')!.click())
  const panel = document.querySelector('[data-browser-panel]')!
  expect(document.querySelector('[data-desktop-panel]')).not.toBeNull()
  const managed = panel.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!
  expect(managed.getAttribute('aria-disabled')).toBe('true')
  expect(panel.querySelector('[data-browser-blocked]')?.textContent).toBe(i18n.t('control.blockedPaused'))
  await act(async () => managed.click())
  expect(open).not.toHaveBeenCalled()
  // 安装引导仍可点（D19 禁用行）
  await act(async () => panel.querySelector<HTMLButtonElement>('[data-browser-action=browserConnect]')!.click())
  expect(panel.querySelector('[data-browser-action=browserInstallExtension]')?.getAttribute('aria-disabled')).toBeNull()
  expect(panel.querySelector('[data-browser-action=browserContinueConnection]')?.getAttribute('aria-disabled')).toBe('true')
})

it('keeps the Agent Screen behind the single header Screen button', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(status)
  await mount()
  const screen = () => document.querySelector('[data-agent-screen]')
  expect(screen()?.textContent).toContain("Agent's screen")
  await act(async () => document.querySelector<HTMLButtonElement>('[data-agent-screen] [aria-label="Hide preview"]')!.click())
  expect(screen()).toBeNull()
  const reopen = header().querySelector<HTMLButtonElement>('[data-header-screen]')!
  expect(document.activeElement).toBe(reopen)
  expect(document.querySelectorAll('.agent-screen-reopen')).toHaveLength(0)
  await act(async () => reopen.click())
  expect(screen()).not.toBeNull()
})
