// owner-followups-1015 #15：发布确认卡（components D8「发布确认」）、⌘K「发布…」（D18：0 个灰显写因 / 1 个直发 /
// 多个先选）、托管报告「配置发布目标」与发布卡「打开设置·发布」→ 设置·发布并定位到目标。
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api, type PendingQuestion, type ReleaseTarget } from './api'
import { useUiStore } from './store'
import { handlePendingKey } from './decisions'
import { isMac } from './keymap'
import { runManagedNext } from './managedNext'
import { CommandPalette } from './components/CommandPalette'
import { PendingCard } from './components/PendingCards'
import { ReleaseSection } from './components/settings/ReleaseSection'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const t = (k: string, o?: Record<string, unknown>) => i18n.t(k, o) as string
const card: PendingQuestion = {
  id: 'q9', kind: 'release', agent_id: null, state: 'queued',
  payload: { target_id: 'r1', name: 'npm', command: 'npm publish --access public', cwd: 'packages/web', credentials: ['NPM_TOKEN'], requested_by: 'owner' },
}
const npm: ReleaseTarget = { id: 'r1', name: 'npm', command: 'npm publish', cwd: '', credentials: [{ name: 'NPM_TOKEN', set: true }] }
const pages: ReleaseTarget = { id: 'r2', name: 'Pages', command: 'npm run deploy', cwd: '', credentials: [] }
const approveKey = () => new KeyboardEvent('keydown', { key: 'Enter', metaKey: isMac, ctrlKey: !isMac, cancelable: true })

let el: HTMLDivElement
let root: Root
let saved: ReturnType<typeof useUiStore.getState>
beforeEach(async () => {
  await i18n.changeLanguage('en')
  saved = useUiStore.getState()
  useUiStore.setState({ modalScope: 'workbench', team: [], artifacts: [], pending: [], tabs: [], activeTab: null, invalidate: async () => {} })
  localStorage.removeItem('hexagon.palette.recents')
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
})
afterEach(async () => {
  await act(async () => root.unmount()); el.remove(); vi.restoreAllMocks()
  useUiStore.setState(saved, true)
})

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })

describe('release confirmation card', () => {
  it('shows what will run and confirms only by click, never by the approve key', async () => {
    const confirm = vi.spyOn(api, 'confirmRelease').mockResolvedValue({ target_id: 'r1', name: 'npm', output: '', duration_ms: 1 })
    await act(async () => root.render(<PendingCard q={card} top />))
    expect(el.textContent).toContain(t('cards.release'))
    expect(el.textContent).toContain('npm publish --access public')
    expect(el.textContent).toContain('packages/web')
    expect(el.textContent).toContain('NPM_TOKEN')
    expect(el.textContent).toContain(t('cards.releaseWarn'))
    // Never remembered: no "allow for this project" anywhere on the card.
    expect(el.querySelector('input[type="checkbox"]')).toBeNull()
    expect(el.querySelector('[data-remember]')).toBeNull()

    expect(await handlePendingKey(approveKey(), [card])).toBe('blocked-release')
    expect(confirm).not.toHaveBeenCalled()

    const button = el.querySelector<HTMLButtonElement>('[data-release-confirm]')!
    expect(button.textContent).toBe(t('cards.releaseConfirm'))
    await act(async () => button.click())
    await flush()
    expect(confirm).toHaveBeenCalledExactlyOnceWith('q9')
  })

  it('reads in Chinese (non-English smoke)', async () => {
    await i18n.changeLanguage('zh-CN')
    await act(async () => root.render(<PendingCard q={card} top />))
    expect(el.textContent).toContain('发布确认')
    expect(el.querySelector('[data-release-confirm]')?.textContent).toBe('确认发布')
    expect(el.textContent).toContain('打开设置·发布')
  })

  it('opens Settings · Release located at the card target', async () => {
    const sections: unknown[] = []
    const listen = (e: Event) => sections.push((e as CustomEvent).detail)
    window.addEventListener('hexagon:open-settings-section', listen)
    try {
      await act(async () => root.render(<PendingCard q={card} top />))
      await act(async () => el.querySelector<HTMLButtonElement>('[data-release-open]')!.click())
      expect(sections).toEqual(['release'])
      // Settings mounts afterwards and picks the pending focus up.
      vi.spyOn(api, 'releaseTargets').mockResolvedValue([pages, npm])
      await act(async () => root.render(<ReleaseSection />))
      await flush()
      const selected = el.querySelector('[role="option"][aria-selected="true"]')
      expect(selected?.textContent).toContain('npm')
    } finally { window.removeEventListener('hexagon:open-settings-section', listen) }
  })
})

describe('⌘K Release…', () => {
  const renderPalette = async (targets: ReleaseTarget[]) => {
    vi.spyOn(api, 'releaseTargets').mockResolvedValue(targets)
    const close = vi.fn()
    await act(async () => root.render(<CommandPalette onClose={close} />))
    await flush()
    return close
  }
  const row = () => document.querySelector<HTMLElement>('[data-palette-item="c-release"]')!
  const input = () => document.querySelector<HTMLInputElement>('[data-command-palette] input')!
  const search = async (q: string) => act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input(), q)
    input().dispatchEvent(new Event('input', { bubbles: true }))
  })
  const enter = () => act(async () => { input().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })) })

  it('is disabled with the reason when no target is registered', async () => {
    const request = vi.spyOn(api, 'requestRelease')
    const close = await renderPalette([])
    expect(row().getAttribute('data-disabled')).toBe('true')
    expect(row().textContent).toContain(t('palette.releaseNone'))
    await act(async () => row().click())
    expect(request).not.toHaveBeenCalled()
    expect(close).not.toHaveBeenCalled()
  })

  it('requests the only target directly and opens its card', async () => {
    const request = vi.spyOn(api, 'requestRelease').mockResolvedValue('q9')
    await renderPalette([npm])
    expect(row().textContent).toContain('npm')
    await search('release')
    await enter()
    await flush()
    expect(request).toHaveBeenCalledExactlyOnceWith('r1')
    expect(useUiStore.getState().pendingDialogOpen).toBe(true)
  })

  it('asks which target first when several are registered', async () => {
    const request = vi.spyOn(api, 'requestRelease').mockResolvedValue('q9')
    const close = await renderPalette([npm, pages])
    expect(row().textContent).toContain(t('palette.releaseCount', { count: 2 }))
    await search('release')
    await enter()
    expect(request).not.toHaveBeenCalled()
    expect(close).not.toHaveBeenCalled()
    expect(input().placeholder).toBe(t('palette.releaseChoose'))
    const options = [...document.querySelectorAll('[role="option"]')].map((o) => o.textContent)
    expect(options).toHaveLength(2)
    await search('Pages')
    await enter()
    await flush()
    expect(request).toHaveBeenCalledExactlyOnceWith('r2')
    expect(close).toHaveBeenCalled()
  })
})

describe('delegation report next step', () => {
  it('“Configure release target” opens Settings · Release located at the target', async () => {
    const sections: unknown[] = []
    const listen = (e: Event) => sections.push((e as CustomEvent).detail)
    window.addEventListener('hexagon:open-settings-section', listen)
    try {
      vi.spyOn(api, 'releaseTargets').mockResolvedValue([npm, pages])
      await act(async () => root.render(<ReleaseSection />))
      await flush()
      expect(el.querySelector('[role="option"][aria-selected="true"]')).toBeNull()
      // Already mounted: the focus event selects the row.
      await act(async () => runManagedNext({ kind: 'release_targets', target: 'r2' }, 7))
      expect(sections).toEqual(['release'])
      expect(el.querySelector('[role="option"][aria-selected="true"]')?.textContent).toContain('Pages')
      expect(t('delegation.nextAction.release_targets')).toBe('Configure release target')
    } finally { window.removeEventListener('hexagon:open-settings-section', listen) }
  })
})
