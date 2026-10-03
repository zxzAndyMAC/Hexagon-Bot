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
