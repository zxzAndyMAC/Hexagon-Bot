import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { api, type StageEvidence as Evidence } from './api'
import { StageEvidence } from './components/StageEvidence'
import { useUiStore } from './store'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it('coalesces invalidations while an expensive evidence read is in flight', async () => {
  const saved = useUiStore.getState()
  const resolvers: ((value: Evidence) => void)[] = []
  const read = vi.spyOn(api, 'stageEvidence').mockImplementation(() => new Promise((resolve) => resolvers.push(resolve)))
  const el = document.createElement('div')
  const root = createRoot(el)
  const evidence: Evidence = { run_id: 'run', fingerprint: 'v1:A', checks: [], missing: ['artifact:old'], exceptions: [] }
  try {
    await act(async () => root.render(<StageEvidence runId="run" />))
    for (let i = 0; i < 12; i++) {
      await act(async () => useUiStore.setState((s) => ({ evidenceRevision: s.evidenceRevision + 1 })))
    }
    // 2026-10-01 live acceptance: every event used to launch another full
    // worktree hash while the previous read was still running.
    expect(read).toHaveBeenCalledTimes(1)
    await act(async () => resolvers[0](evidence))
    expect(read).toHaveBeenCalledTimes(2)
    expect(el.textContent).not.toContain('old')
    await act(async () => resolvers[1]({ ...evidence, missing: ['artifact:latest'] }))
    expect(el.textContent).toContain('latest')
    await act(async () => window.dispatchEvent(new Event('focus')))
    expect(read).toHaveBeenCalledTimes(3)
    await act(async () => root.unmount())
    await act(async () => resolvers[2](evidence))
    expect(read).toHaveBeenCalledTimes(3)
  } finally {
    await act(async () => root.unmount())
    vi.restoreAllMocks()
    useUiStore.setState(saved, true)
  }
})
