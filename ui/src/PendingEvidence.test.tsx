import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api } from './api'
import { useUiStore } from './store'
import { PendingCards } from './components/PendingCards'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

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
