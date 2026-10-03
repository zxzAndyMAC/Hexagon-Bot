import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { VirtuosoMockContext } from 'react-virtuoso'
import './i18n'
import { api, type SkillRow } from './api'
import { bindingFor, formatBinding, resetBinding, setBinding, isMac } from './keymap'
import { SettingsPage } from './components/SettingsPage'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
let root: Root
let el: HTMLDivElement
const row = (name: string): SkillRow => ({ name, description: `description ${name}`, origin: 'global', enabled: true })
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(r => { resolve = r })
  return { promise, resolve }
}
async function click(text: string) {
  const node = [...el.querySelectorAll<HTMLElement>('button,[role="button"]')].find(node => node.textContent?.trim() === text)
    ?? [...el.querySelectorAll<HTMLElement>('button,[role="button"]')].find(node => node.textContent?.includes(text))
  expect(node).toBeTruthy()
  await act(async () => node!.click())
}
async function input(node: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(node, value)
    node.dispatchEvent(new Event('input', { bubbles: true }))
  })
}
async function mount() {
  await act(async () => root.render(<VirtuosoMockContext.Provider value={{ viewportHeight: 480, itemHeight: 60 }}>
    <SettingsPage onBack={() => {}} projectless />
  </VirtuosoMockContext.Provider>))
  await click('Skills')
}
beforeEach(() => {
  el = document.createElement('div'); document.body.append(el); root = createRoot(el)
  vi.spyOn(api, 'logEnabled').mockResolvedValue(true)
  vi.spyOn(api, 'skillFiles').mockResolvedValue(['SKILL.md'])
  vi.spyOn(api, 'readSkillFile').mockResolvedValue('fixture body')
})
afterEach(async () => {
  await act(async () => root.unmount()); el.remove(); vi.restoreAllMocks()
})

it('renders a bounded 1024-skill list and searches unmounted names, preserving switches and keyboard reach', async () => {
  vi.spyOn(api, 'listSkills').mockResolvedValue(Array.from({ length: 1024 }, (_, i) => row(`fixture-${String(i).padStart(4, '0')}`)))
  const muted = vi.spyOn(api, 'setSkillMuted').mockResolvedValue(undefined)
  const start = performance.now()
  await mount()
  const mountMs = performance.now() - start
  const rendered = el.querySelectorAll('[data-item-index]').length
  expect(rendered).toBeGreaterThan(0)
  expect(rendered).toBeLessThan(25)
  expect(el.textContent).not.toContain('fixture-1023')
  const first = el.querySelector<HTMLElement>('[data-item-index] [role="button"]')!
  await act(async () => first.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true })))
  expect(api.skillFiles).toHaveBeenLastCalledWith('fixture-1023')
  const searchStart = performance.now()
  await input(el.querySelector<HTMLInputElement>('input[placeholder="Filter skills…"]')!, 'fixture-1023')
  expect(el.textContent).toContain('fixture-1023')
  expect(el.textContent).not.toContain('fixture-0000')
  const checkbox = el.querySelector<HTMLInputElement>('input[type="checkbox"]')!
  await act(async () => checkbox.click())
  expect(muted).toHaveBeenCalledWith('fixture-1023', false)
  console.info(`skills-ui-layout-stub n=1024 mount_ms=${mountMs.toFixed(2)} search_and_toggle_ms=${(performance.now() - searchStart).toFixed(2)} mounted_rows=${rendered}; excludes disk/IPC and native layout`)
})

it('keeps loading visible and rejects a late file/tree response after selecting another skill', async () => {
  vi.spyOn(api, 'listSkills').mockResolvedValue([row('alpha'), row('beta')])
  const oldFile = deferred<string>()
  const oldTree = deferred<string[]>()
  vi.mocked(api.readSkillFile).mockImplementation(name => name === 'alpha' ? oldFile.promise : Promise.resolve('beta body'))
  vi.mocked(api.skillFiles).mockImplementation(name => name === 'alpha' ? oldTree.promise : Promise.resolve(['SKILL.md']))
  await mount(); await click('alpha')
  expect(el.textContent).toContain('Loading skill file…')
  await click('beta')
  expect(el.textContent).toContain('beta body')
  await act(async () => { oldFile.resolve('ALPHA OLD BODY'); oldTree.resolve(['SKILL.md', 'ALPHA-OLD.txt']) })
  expect(el.textContent).toContain('beta body')
  expect(el.textContent).not.toContain('ALPHA OLD BODY')
  expect(el.textContent).not.toContain('ALPHA-OLD.txt')
})

it('keeps the newest list when an initial read arrives after a create refresh', async () => {
  const old = deferred<SkillRow[]>()
  vi.spyOn(api, 'listSkills').mockReturnValueOnce(old.promise).mockResolvedValue([row('created-skill')])
  vi.spyOn(api, 'saveGlobalSkill').mockResolvedValue(undefined)
  await mount()
  expect(el.textContent).toContain('Loading skills…')
  await click('+ New skill')
  await input(el.querySelector<HTMLInputElement>('input[placeholder="my-skill"]')!, 'created-skill')
  await click('Create')
  expect(el.textContent).toContain('created-skill')
  await act(async () => old.resolve([row('stale-list-entry')]))
  expect(el.textContent).toContain('created-skill')
  expect(el.textContent).not.toContain('stale-list-entry')
})

it('offers a retry after list failure without claiming there are no skills', async () => {
  vi.spyOn(api, 'listSkills').mockRejectedValueOnce(new Error('fixture read failed')).mockResolvedValue([row('recovered')])
  await mount()
  expect(el.querySelector('[role="alert"]')?.textContent).toContain('fixture read failed')
  await click('Retry loading')
  expect(el.textContent).toContain('recovered')
  expect(el.querySelector('[role="alert"]')).toBeNull()
})

it('registers the skills retry action and honors its displayed custom shortcut', async () => {
  const list = vi.spyOn(api, 'listSkills').mockRejectedValueOnce(new Error('fixture read failed')).mockResolvedValue([row('recovered-by-key')])
  setBinding('retrySkillsLoad', 'alt+mod+r')
  try {
    await mount()
    const button = el.querySelector<HTMLButtonElement>('[role="alert"] button')!
    expect(button.title).toContain(formatBinding(bindingFor('retrySkillsLoad')))
    await act(async () => button.dispatchEvent(new KeyboardEvent('keydown', { key: 'r', code: 'KeyR', altKey: true, ctrlKey: !isMac, metaKey: isMac, bubbles: true })))
    expect(list).toHaveBeenCalledTimes(2)
    expect(el.textContent).toContain('recovered-by-key')
  } finally { resetBinding('retrySkillsLoad') }
})
