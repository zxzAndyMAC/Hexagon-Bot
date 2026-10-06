import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserControlPanel } from './components/BrowserControlPanel'
import { api } from './api'
import i18n from './i18n'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(async () => { await act(async () => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })
const desktop = { project_root: '/a', enabled: true, active_project: null, active_agent: null, busy: false, paused: false, outcome_unknown: false, screenshot_count: 0 }
const browser = { project_root: '/a', session_id: 's', mode: 'managed' as const, tab_id: 't', navigation_generation: 1, url: 'about:blank', title: '', connected: true }
it('opens managed by explicit choice; extension is independent and detach carries project identity', async () => {
  await i18n.changeLanguage('zh-CN')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  vi.spyOn(api, 'browserStatus').mockResolvedValue(null)
  const open = vi.spyOn(api, 'browserOpen').mockResolvedValue(browser)
  const detach = vi.spyOn(api, 'browserDetach').mockResolvedValue()
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel />))
  expect(open).not.toHaveBeenCalled()
  await act(async () => el.querySelector('button')!.click())
  expect(el.textContent).toContain('不会关闭已有标签页')
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.click())
  expect(open).toHaveBeenCalledExactlyOnceWith('/a', 'managed', expect.objectContaining({ element: expect.any(String) }))
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserDetach]')!.click())
  expect(detach).toHaveBeenCalledExactlyOnceWith('/a', 's')
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserConnect]')!.click())
  // Owner issue16: connection intent first exposes installation/permission guidance.
  expect(open).toHaveBeenCalledTimes(1)
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserContinueConnection]')!.click())
  expect(open).toHaveBeenLastCalledWith('/a', 'extension', expect.objectContaining({ element: expect.any(String) }))
})
it('a late session response cannot reopen a closed workspace UI', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  vi.spyOn(api, 'browserStatus').mockResolvedValue(null)
  let resolve!: (value: typeof browser) => void
  vi.spyOn(api, 'browserOpen').mockImplementation(() => new Promise(r => { resolve = r }))
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel />))
  await act(async () => el.querySelector('button')!.click())
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.click())
  await act(async () => root.render(null))
  await act(async () => resolve(browser))
  expect(el.childElementCount).toBe(0)
})
it('cannot open a browser while project consent is disabled or computer paused', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue({ ...desktop, paused: true })
  vi.spyOn(api, 'browserStatus').mockResolvedValue(null)
  const open = vi.spyOn(api, 'browserOpen')
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel />))
  await act(async () => el.querySelector('button')!.click())
  const managed = el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!
  expect(managed.disabled).toBe(true)
  await act(async () => managed.click())
  expect(open).not.toHaveBeenCalled()
  // Setup and installation remain available while paused; actual connection does not.
  await act(async()=>el.querySelector<HTMLButtonElement>('[data-browser-action=browserConnect]')!.click())
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserContinueConnection]')!.disabled).toBe(true)
  expect(open).not.toHaveBeenCalled()
})

it('opens the official installation page only on explicit installation intent and permits retry', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  vi.spyOn(api, 'browserStatus').mockResolvedValue(null)
  const connect=vi.spyOn(api, 'browserOpen')
  const store=vi.spyOn(api, 'openBrowserExtensionStore').mockRejectedValueOnce(new Error('open failed')).mockResolvedValue()
  root=createRoot(el);document.body.appendChild(el)
  await act(async()=>root.render(<BrowserControlPanel/>))
  await act(async()=>el.querySelector('button')!.click())
  await act(async()=>el.querySelector<HTMLButtonElement>('[data-browser-action=browserConnect]')!.click())
  expect(store).not.toHaveBeenCalled();expect(connect).not.toHaveBeenCalled()
  expect(el.textContent).toContain(i18n.t('browser.extensionAccess'))
  const install=el.querySelector<HTMLButtonElement>('[data-browser-action=browserInstallExtension]')!
  await act(async()=>install.click())
  expect(store).toHaveBeenCalledTimes(1)
  expect(el.querySelector('[role="alert"]')?.textContent).toContain('open failed')
  await act(async()=>install.click())
  expect(store).toHaveBeenCalledTimes(2);expect(connect).not.toHaveBeenCalled()
  expect(el.querySelector('[role="alert"]')).toBeNull()
})

it('keeps browser setup visible as a Settings section without a popover trigger', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  vi.spyOn(api, 'browserStatus').mockResolvedValue(null)
  const open = vi.spyOn(api, 'browserOpen')
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel inline />))
  expect(el.querySelector('.approval-mode-trigger')).toBeNull()
  expect(el.querySelector('[role="region"]')).not.toBeNull()
  await act(async () => document.body.dispatchEvent(new Event('pointerdown', { bubbles: true })))
  expect(el.querySelector('[role="region"]')).not.toBeNull()
  expect(open).not.toHaveBeenCalled()
})

// Owner 2026-10-06: a closed/failed browser status read used to discard a
// successful consent read, leaving both launch paths permanently grey.
it('keeps browser launch retry available when browser status cannot be read', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  vi.spyOn(api, 'browserStatus').mockRejectedValue(new Error('browser disconnected'))
  const open = vi.spyOn(api, 'browserOpen').mockResolvedValue(browser)
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel inline />))
  expect(el.querySelector('[role="alert"]')?.textContent).toContain('browser disconnected')
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.disabled).toBe(false)
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserConnect]')!.click())
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserContinueConnection]')!.disabled).toBe(false)
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserContinueConnection]')!.click())
  expect(open).toHaveBeenCalledWith('/a', 'extension', expect.anything())
})

it('re-enables launch after browser closure and shows pause or disabled reasons', async () => {
  await i18n.changeLanguage('zh-CN')
  const consent = vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  const session = vi.spyOn(api, 'browserStatus').mockResolvedValue(browser)
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel inline />))
  const managed = () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!
  expect(managed().disabled).toBe(true)
  session.mockResolvedValue({ ...browser, connected: false })
  await act(async () => window.dispatchEvent(new Event('focus')))
  expect(managed().disabled).toBe(false)
  // Owner 2026-10-06: the retained disconnected record is informational;
  // offering detach here used to call a dead worker and show an internal error.
  expect(el.querySelector('[data-browser-action=browserDetach]')).toBeNull()
  for (const state of [{ ...desktop, paused: true }, { ...desktop, enabled: false }]) {
    consent.mockResolvedValue(state)
    await act(async () => window.dispatchEvent(new Event('hexagon:desktop-status-changed')))
    expect(managed().disabled).toBe(true)
    expect(el.querySelector('[role="status"]')!.textContent).toContain(i18n.t(state.enabled ? 'computer.paused' : 'computer.disabled'))
  }
  consent.mockRejectedValue(new Error('consent unavailable'))
  await act(async () => window.dispatchEvent(new Event('focus')))
  expect(managed().disabled).toBe(true)
})

it('keeps a known live session protected when its refresh fails', async () => {
  const consent = vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  const session = vi.spyOn(api, 'browserStatus').mockResolvedValue(browser)
  const detach = vi.spyOn(api, 'browserDetach').mockResolvedValue()
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel inline />))
  session.mockRejectedValue(new Error('transient read failure'))
  await act(async () => window.dispatchEvent(new Event('focus')))
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.disabled).toBe(true)
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserDetach]')!.disabled).toBe(false)
  consent.mockRejectedValue(new Error('consent unavailable'))
  await act(async () => window.dispatchEvent(new Event('focus')))
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.disabled).toBe(true)
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserDetach]')!.click())
  expect(detach).toHaveBeenCalledExactlyOnceWith('/a', 's')
})

it('shows a manually closed browser as disconnected without offering another detach', async () => {
  await i18n.changeLanguage('zh-CN')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  vi.spyOn(api, 'browserStatus').mockResolvedValue({ ...browser, connected: false })
  const detach = vi.spyOn(api, 'browserDetach')
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel inline />))
  expect(el.textContent).toContain(i18n.t('browser.disconnected'))
  expect(el.querySelector('[data-browser-action=browserDetach]')).toBeNull()
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.disabled).toBe(false)
  expect(detach).not.toHaveBeenCalled()
})

it('reconciles a browser closed before the next status poll instead of showing an internal error', async () => {
  await i18n.changeLanguage('zh-CN')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  const read = vi.spyOn(api, 'browserStatus').mockResolvedValue(browser)
  const detach = vi.spyOn(api, 'browserDetach').mockRejectedValue({ code: 'internal', message: 'browser disconnected; explicitly open a new session' })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel inline />))
  // Owner 2026-10-06: closing Chromium leaves a disconnected session record;
  // the stale UI still sends detach, whose liveness rejection is mapped to internal.
  read.mockResolvedValue({ ...browser, connected: false })
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserDetach]')!.click())
  expect(detach).toHaveBeenCalledExactlyOnceWith('/a', 's')
  expect(el.querySelector('[role="alert"]')).toBeNull()
  expect(el.textContent).toContain(i18n.t('browser.disconnected'))
  expect(el.querySelector('[data-browser-action=browserDetach]')).toBeNull()
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.disabled).toBe(false)
})

it('accepts host-confirmed absence after a stale detach failure', async () => {
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  const read = vi.spyOn(api, 'browserStatus').mockResolvedValue(browser)
  vi.spyOn(api, 'browserDetach').mockRejectedValue(new Error('worker gone'))
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel inline />))
  read.mockResolvedValue(null)
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserDetach]')!.click())
  expect(el.querySelector('[role="alert"]')).toBeNull()
  expect(el.querySelector('[data-browser-action=browserDetach]')).toBeNull()
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.disabled).toBe(false)
})

it.each(['connected', 'unreadable', 'replacement', 'other project'])('preserves a detach failure when refreshed status is %s', async state => {
  await i18n.changeLanguage('zh-CN')
  vi.spyOn(api, 'desktopStatus').mockResolvedValue(desktop)
  const read = vi.spyOn(api, 'browserStatus').mockResolvedValue(browser)
  vi.spyOn(api, 'browserDetach').mockRejectedValue({ code: 'internal', message: 'detach failed' })
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<BrowserControlPanel inline />))
  if (state === 'unreadable') read.mockRejectedValue(new Error('status unavailable'))
  else if (state === 'replacement') read.mockResolvedValue({ ...browser, session_id: 'replacement', connected: false })
  else if (state === 'other project') read.mockResolvedValue({ ...browser, project_root: '/other', connected: false })
  await act(async () => el.querySelector<HTMLButtonElement>('[data-browser-action=browserDetach]')!.click())
  expect(el.querySelector('[role="alert"]')!.textContent).toBe(i18n.t('errors.internal'))
  expect(el.querySelector('[data-browser-action=browserDetach]')).not.toBeNull()
  expect(el.querySelector<HTMLButtonElement>('[data-browser-action=browserManaged]')!.disabled).toBe(true)
})
