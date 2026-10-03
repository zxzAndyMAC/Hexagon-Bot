import { assertModalBlocksIntake } from './modalDecisionTestHelpers'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { DesktopToolDetails } from './components/DesktopToolDetails'
import { api } from './api'
import i18n from './i18n'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
it('loads screenshots on demand and clearing prevents an in-flight response from restoring the image', async () => {
  await i18n.changeLanguage('en')
  let resolve!: (v: {name: string; data_url: string}) => void
  const read = vi.spyOn(api, 'desktopScreenshot').mockImplementation(() => new Promise(r => { resolve = r }))
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<DesktopToolDetails output={{ result: { screenshot_path: '/project/.hexagon/computer-use/screenshots/abc.png' }, requires_observation: true }} />))
    expect(read).not.toHaveBeenCalled()
    expect(el.textContent).toContain('observe again')
    await act(async () => el.querySelector('button')!.click())
    expect(read).toHaveBeenCalledWith('abc.png', '/project')
    await act(async () => window.dispatchEvent(new Event('hexagon:screenshots-cleared')))
    await act(async () => resolve({name:'abc.png', data_url:'data:image/png;base64,old'}))
    expect(el.querySelector('img')).toBeNull()
    expect(el.textContent).toContain('unavailable or cleared')
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})
it('discards the previous image and delayed response when a row is reused for another screenshot', async () => {
  let resolve!: (v: {name: string; data_url: string}) => void
  vi.spyOn(api, 'desktopScreenshot').mockImplementation(() => new Promise(r => { resolve = r }))
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<DesktopToolDetails output={{ result: { screenshot_path: '/old/.hexagon/computer-use/screenshots/a.png' } }} />))
    await act(async () => el.querySelector('button')!.click())
    await act(async () => root.render(<DesktopToolDetails output={{ result: { screenshot_path: '/new/.hexagon/computer-use/screenshots/b.png' } }} />))
    await act(async () => resolve({ name: 'a.png', data_url: 'data:image/png;base64,old' }))
    expect(el.querySelector('img')).toBeNull()
    expect(el.querySelector('button')!.disabled).toBe(false)
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})

it('waits for visibility, opens a bounded thumbnail in a modal, blocks decisions and returns focus', async () => {
  const callbacks: IntersectionObserverCallback[] = []
  vi.stubGlobal('IntersectionObserver', class {
    constructor(callback: IntersectionObserverCallback) { callbacks.push(callback) }
    observe() {} disconnect() {}
  })
  const read = vi.spyOn(api, 'desktopScreenshot').mockResolvedValue({ name: 'a.png', data_url: 'data:image/png;base64,new' })
  const el = document.createElement('div'); document.body.append(el); const root = createRoot(el)
  const background = vi.fn()
  try {
    await act(async () => root.render(<DesktopToolDetails projectId="p1" capturedAt="2026-10-02T01:00:00Z" output={{ result: { screenshot_path: '/p/.hexagon/computer-use/screenshots/a.png', bundle_id: 'com.google.Chrome', title: 'Test window' } }} />))
    expect(read).not.toHaveBeenCalled()
    await act(async () => callbacks[0]([{ isIntersecting: false } as IntersectionObserverEntry], {} as IntersectionObserver))
    expect(read).not.toHaveBeenCalled()
    await act(async () => callbacks[0]([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver))
    expect(read).toHaveBeenCalledTimes(1)
    const trigger = el.querySelector<HTMLButtonElement>('.desktop-screenshot-thumbnail')!
    expect(trigger.querySelector('img')).not.toBeNull()
    expect(el.textContent).toContain('com.google.Chrome')
    await act(async () => trigger.click())
    const dialog = el.querySelector('dialog')!
    expect(dialog.open).toBe(true)
    expect(document.activeElement).toBe(dialog.querySelector('button'))
    window.addEventListener('keydown', background)
    const { isMac } = await import('./keymap')
    background.mockClear()
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: !isMac, metaKey: isMac, bubbles: true })))
    expect(background).not.toHaveBeenCalled()
    await assertModalBlocksIntake()
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(el.querySelector('dialog')).toBeNull()
    expect(document.activeElement).toBe(trigger)
    await act(async () => trigger.click())
    await act(async () => window.dispatchEvent(new Event('hexagon:screenshots-cleared')))
    expect(el.querySelector('dialog')).toBeNull()
    expect(el.querySelector('img')).toBeNull()
    expect(trigger.disabled).toBe(true)
  } finally {
    window.removeEventListener('keydown', background)
    await act(async () => root.unmount()); el.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals()
  }
})

it('isolates a late read across workspaces sharing p1 and the same filename', async () => {
  const resolvers: Array<(v: { name: string; data_url: string }) => void> = []
  const read = vi.spyOn(api, 'desktopScreenshot').mockImplementation(() => new Promise(resolve => resolvers.push(resolve)))
  const el = document.createElement('div'); const root = createRoot(el)
  const output = (root: string) => ({ result: { screenshot_path: `${root}/.hexagon/computer-use/screenshots/same.png` } })
  try {
    await act(async () => root.render(<DesktopToolDetails projectId="p1" output={output('/old')} />))
    await act(async () => el.querySelector('button')!.click())
    await act(async () => root.render(<DesktopToolDetails projectId="p1" output={output('/new')} />))
    await act(async () => el.querySelector('button')!.click())
    expect(read).toHaveBeenNthCalledWith(1, 'same.png', '/old')
    expect(read).toHaveBeenNthCalledWith(2, 'same.png', '/new')
    await act(async () => resolvers[0]({ name: 'same.png', data_url: 'data:image/png;base64,old' }))
    expect(el.querySelector('img')).toBeNull()
    await act(async () => resolvers[1]({ name: 'same.png', data_url: 'data:image/png;base64,new' }))
    expect(el.querySelector('img')?.getAttribute('src')).toContain('new')
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})
