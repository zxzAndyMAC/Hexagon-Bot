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

it('keeps a high-score policy candidate pending until owner adoption and opens its full report', async () => {
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
    expect(el.textContent).toContain('does not prove task quality')
    expect(accept).not.toHaveBeenCalled()
    const report = [...el.querySelectorAll('button')].find((b) => b.textContent === 'View candidate and replay report')!
    expect(report.title).toContain('P')
    await act(async () => report.click())
    expect(openTab).toHaveBeenCalledWith(expect.objectContaining({ kind: 'artifact', path: 'proposals/policy.md' }))
    const adopt = [...el.querySelectorAll('button')].find((b) => b.textContent?.startsWith('Adopt candidate'))!
    await act(async () => adopt.click())
    expect(accept).toHaveBeenCalledExactlyOnceWith('policy-card')
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
