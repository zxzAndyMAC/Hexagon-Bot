import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { PendingCard } from './components/PendingCards'
import { handlePendingKey } from './decisions'
import { isMac } from './keymap'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const card: PendingQuestion = { id: 'policy-card', kind: 'stamp', agent_id: 'a0', state: 'queued', payload: {
  proposal_id: 'policy-1', surface: 'pack_copy', policy_candidate: true,
  evidence: { baseline_score: 0, candidate_score: 20, scenario: 'fixed' },
} }

// Evaluation 16/D14: old scores cannot authorize adoption; viewing stays available.
it('keeps an unverified high-score candidate pending and opens its full report', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  const openTab = vi.fn()
  useUiStore.setState({ invalidate: async () => {}, modalScope: 'workbench', openTab })
  const accept = vi.spyOn(api, 'confirmProposal').mockResolvedValue('policy-1')
  vi.spyOn(api, 'proposals').mockResolvedValue([{ id: 'policy-1', surface: 'pack_copy', target: '.hexagon/pack.active.json',
    status: 'awaiting_stamp', author: 'a0', artifact_path: 'proposals/policy.md' }])
  const el = document.createElement('div'); document.body.append(el)
  const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCard q={card} top />))
    expect(el.textContent).toContain('Policy candidate')
    expect(el.textContent).toContain('current 0 · candidate 20')
    expect(el.textContent).toContain('Independent quality evidence is missing')
    expect(accept).not.toHaveBeenCalled()
    const report = [...el.querySelectorAll('button')].find((b) => b.textContent === 'View candidate and replay report')!
    expect(report.title).toContain('P')
    await act(async () => report.click())
    expect(openTab).toHaveBeenCalledWith(expect.objectContaining({ kind: 'artifact', path: 'proposals/policy.md' }))
    const adopt = [...el.querySelectorAll('button')].find((b) => b.textContent?.startsWith('Adopt candidate'))!
    expect(adopt.disabled).toBe(true)
    await act(async () => adopt.click())
    const event = new KeyboardEvent('keydown', { key: 'Enter', metaKey: isMac, ctrlKey: !isMac })
    expect(await handlePendingKey(event, [card])).toBe('blocked-policy')
    expect(accept).not.toHaveBeenCalled()
  } finally { await act(async () => root.unmount()); el.remove(); vi.restoreAllMocks(); useUiStore.setState(saved, true) }
})

it('blocks unresolved policy recovery through both the button and approval shortcut', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ modalScope: 'workbench' })
  const accept = vi.spyOn(api, 'confirmProposal')
  const pending = { ...card, payload: { ...card.payload, policy_recovery: true } }
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCard q={pending} top />))
    expect(el.querySelector<HTMLButtonElement>('button.primary')?.disabled).toBe(true)
    expect(el.textContent).toContain('Current files are preserved')
    const event = new KeyboardEvent('keydown', { key: 'Enter', metaKey: isMac, ctrlKey: !isMac })
    expect(await handlePendingKey(event, [pending])).toBe('blocked-policy')
    expect(accept).not.toHaveBeenCalled()
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); useUiStore.setState(saved, true) }
})

// Benchmark I1 / review P0: an old read cannot open a tab or dismiss the new
// project's decision dialog, including reopening the same canonical root.
it('discards a candidate report that arrives after a project switch', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ projectRoot: '/repo/a', projectEpoch: 71, modalScope: 'workbench' })
  let resolve!: (value: Awaited<ReturnType<typeof api.proposals>>) => void
  vi.spyOn(api, 'proposals').mockImplementation(() => new Promise(r => { resolve = r }))
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCard q={card} top />))
    const button = [...el.querySelectorAll('button')].find(b => b.textContent === 'View candidate and replay report')!
    await act(async () => button.click())
    await act(async () => {
      useUiStore.getState().beginProjectSwitch()
      useUiStore.setState({ projectRoot: '/repo/a', pendingDialogOpen: true })
      resolve([{ id: 'policy-1', surface: 'pack_copy', target: '.hexagon/pack.active.json',
        status: 'awaiting_stamp', author: 'a0', artifact_path: 'proposals/old.md' }])
    })
    expect(useUiStore.getState().tabs.map(t => t.id)).toEqual(['timeline'])
    expect(useUiStore.getState().pendingDialogOpen).toBe(true)
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); useUiStore.setState(saved, true) }
})

// Evaluation 18/D14: status refresh updates both the visible reason and action.
it('refreshes all quality states and allows only qualified owner adoption', async () => {
  await i18n.changeLanguage('zh-CN')
  const saved = useUiStore.getState()
  useUiStore.setState({ invalidate: async () => {}, modalScope: 'workbench' })
  const accept = vi.spyOn(api, 'confirmProposal').mockResolvedValue('policy-1')
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    for (const [state, phrase] of [
      ['incomplete', '原定验收尚未完成'], ['failed', '原定验收未通过'],
      ['stale', '候选或评测绑定已变化'], ['qualified', '独立质量验收已通过'],
      ['unverified', '缺少独立质量证据'],
    ]) {
      const pending = { ...card, payload: { ...card.payload, policy_quality: state } }
      await act(async () => root.render(<PendingCard q={pending} top />))
      expect(el.querySelector('[role="status"]')?.textContent).toContain(phrase)
      expect(el.querySelector<HTMLButtonElement>('button.primary')?.disabled).toBe(state !== 'qualified')
      expect(accept).not.toHaveBeenCalled()
    }
    const qualified = { ...card, payload: { ...card.payload, policy_quality: 'qualified' } }
    await act(async () => root.render(<PendingCard q={qualified} top />))
    await act(async () => el.querySelector<HTMLButtonElement>('button.primary')!.click())
    expect(accept).toHaveBeenCalledWith('policy-card')
    accept.mockClear()
    useUiStore.setState({ modalScope: 'settings' })
    const event = () => new KeyboardEvent('keydown', { key: 'Enter', metaKey: isMac, ctrlKey: !isMac })
    await handlePendingKey(event(), [qualified])
    expect(accept).not.toHaveBeenCalled()
    useUiStore.setState({ modalScope: 'workbench' })
    await handlePendingKey(event(), [qualified])
    expect(accept).toHaveBeenCalledWith('policy-card')
  } finally {
    await act(async () => root.unmount()); vi.restoreAllMocks(); useUiStore.setState(saved, true)
    await i18n.changeLanguage('en')
  }
})
