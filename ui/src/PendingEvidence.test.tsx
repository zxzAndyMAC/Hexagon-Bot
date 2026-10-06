import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api } from './api'
import type { ProposalRow } from './gen/ProposalRow'
import { useUiStore } from './store'
import { PendingCard, PendingCards } from './components/PendingCards'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it('keeps an effective refusal distinct from a task waiting for a configured model', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ invalidate: async () => {}, pending: [{ id: 'q-resume', kind: 'stall', agent_id: 'a1', state: 'queued', payload: {
    branch: 'no_reply', retry: true, source: 'activation_resume', reason: 'resume_provider_unavailable',
    trigger_turn_id: 8, activation_root_turn_id: 2,
  } }] })
  const retry = vi.spyOn(api, 'stallRetry').mockResolvedValue()
  const el = document.createElement('div')
  document.body.append(el)
  const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCard q={useUiStore.getState().pending[0]!} top />))
    expect(el.textContent).toContain('Decision applied; original task waiting')
    expect(el.textContent).toContain('Configure a model, then retry to continue the original task')
    expect(el.textContent).not.toContain('after one retrigger')
    expect(el.textContent).not.toContain('once per stall')
    const button = [...el.querySelectorAll('button')].find((b) => b.textContent?.startsWith('Try again'))
    expect(button).toBeTruthy()
    await act(async () => button?.click())
    expect(retry).toHaveBeenCalledWith('q-resume')
  } finally {
    await act(async () => root.unmount())
    el.remove()
    vi.restoreAllMocks()
    useUiStore.setState(saved, true)
  }
})

it('shows stale checks with original exit codes in the acceptance card', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ pending: [{ id: 'q-evidence', kind: 'stamp', agent_id: null, payload: {
    stage: 'accept', run_id: 'r-final', final_acceptance: true,
  }, state: 'queued' }] })
  vi.spyOn(api, 'stageEvidence').mockResolvedValue({
    exceptions: [], run_id: 'r-final', fingerprint: 'v1:current', missing: ['check:verify'],
    checks: [{ run_id: 'r-build', stage: 'build', cmd: 'verify', event_id: 7, exit_code: 0, state: 'stale' }],
  })
  const el = document.createElement('div')
  document.body.append(el)
  const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCards />))
    expect(el.textContent).toContain('Outdated evidence')
    expect(el.textContent).toContain('verify')
    expect(el.textContent).toContain('Recorded exit code: 0')
    expect(el.textContent).not.toContain('Passed for current delivery')
  } finally {
    await act(async () => root.unmount())
    el.remove()
    vi.restoreAllMocks()
    useUiStore.setState(saved, true)
  }
})

it('refreshes evidence after acceptance is refused without a new timeline event', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({
    pending: [{ id: 'q-evidence', kind: 'stamp', agent_id: null, state: 'queued', payload: {
      stage: 'accept', run_id: 'r-final', final_acceptance: true,
    } }],
    refreshFast: async () => {}, refreshSlow: async () => {},
  })
  const evidence = vi.spyOn(api, 'stageEvidence')
  evidence.mockResolvedValueOnce({ exceptions: [], run_id: 'r-final', fingerprint: 'v1:A', missing: [], checks: [
    { run_id: 'r-build', stage: 'build', cmd: 'verify', event_id: 7, exit_code: 0, state: 'passed' },
  ] }).mockResolvedValue({ exceptions: [], run_id: 'r-final', fingerprint: 'v1:B', missing: ['check:verify'], checks: [
    { run_id: 'r-build', stage: 'build', cmd: 'verify', event_id: 7, exit_code: 0, state: 'stale' },
  ] })
  vi.spyOn(api, 'stamp').mockResolvedValue({ action: 'incomplete', stage: 'accept', missing: ['check:verify'] })
  const el = document.createElement('div')
  document.body.append(el)
  const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCards />))
    expect(el.textContent).toContain('Passed for current delivery')
    const confirm = [...el.querySelectorAll('button')].find((b) => b.textContent?.startsWith('Confirm'))
    expect(confirm).toBeTruthy()
    await act(async () => confirm?.click())
    expect(el.textContent).toContain('Outdated evidence')
    expect(el.textContent).not.toContain('Passed for current delivery')
  } finally {
    await act(async () => root.unmount())
    el.remove()
    vi.restoreAllMocks()
    useUiStore.setState(saved, true)
  }
})

// Benchmark I1: both awaits in proposals -> artifactContent belong to the
// initiating project. A delayed first read must not issue its second read in B.
it.each(['proposal', 'artifact'] as const)('discards a late %s diff after switching projects', async (delay) => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ projectRoot: '/repo/a', projectEpoch: 81, invalidate: async () => {} })
  const proposal: ProposalRow[] = [{ id: 'p1', surface: 'pack_copy', target: 'pack.json', status: 'awaiting_stamp',
    author: 'a0', artifact_path: 'proposals/old.md' }]
  let finishProposal!: (value: typeof proposal) => void
  let finishArtifact!: (value: string) => void
  const proposals = vi.spyOn(api, 'proposals').mockResolvedValue(proposal)
  const content = vi.spyOn(api, 'artifactContent').mockResolvedValue('```diff\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n```')
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCard q={{ id: 'q1', kind: 'stamp', agent_id: 'a0', state: 'queued',
      payload: { proposal_id: 'p1', surface: 'pack_copy' } }} top />))
    await act(async () => [...el.querySelectorAll('button')].find(b => b.textContent?.trim() === i18n.t('cards.inlineDiff'))!.click())
    expect(el.textContent).toContain(i18n.t('cards.openFullDiff'))
    if (delay === 'proposal') proposals.mockImplementationOnce(() => new Promise(r => { finishProposal = r }))
    else content.mockImplementationOnce(() => new Promise(r => { finishArtifact = r }))
    const reads = content.mock.calls.length
    await act(async () => [...el.querySelectorAll('button')].find(b => b.textContent?.trim() === i18n.t('cards.openFullDiff'))!.click())
    await act(async () => {
      useUiStore.getState().beginProjectSwitch()
      useUiStore.setState({ projectRoot: '/repo/b' })
      if (delay === 'proposal') finishProposal(proposal)
      else finishArtifact('```diff\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n```')
    })
    expect(useUiStore.getState().tabs.map(t => t.id)).toEqual(['timeline'])
    if (delay === 'proposal') expect(content).toHaveBeenCalledTimes(reads)
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); useUiStore.setState(saved, true) }
})
