import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { api, type StageEvidence as Evidence } from './api'
import i18n from './i18n'
import { StageEvidence } from './components/StageEvidence'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it.each(['baseline_established', 'regressed'])('shows %s as measured workload evidence without inventing browser metrics', async (reason) => {
  await i18n.changeLanguage('en')
  const evidence: Evidence = { run_id: 'run', fingerprint: 'v1:A', exceptions: [],
    missing: reason === 'regressed' ? ['check:quality:performance'] : [],
    // Live event 56: a process exit of 0 produced the quality verdict -2.
    // Never present that synthetic verdict as the process's original exit code.
    checks: [{ run_id: 'run', stage: 'build', cmd: 'quality:performance', event_id: 5, exit_code: reason === 'regressed' ? -2 : 0,
      state: reason === 'regressed' ? 'failed' : 'passed', quality: {
        category: 'performance', command: 'npm run benchmark', changed_paths: ['ui/src/App.tsx'], reason,
        baseline_event_id: reason === 'regressed' ? 1 : null,
        baseline_ms: reason === 'regressed' ? 100 : null, measured_ms: 250,
      } }],
  }
  vi.spyOn(api, 'stageEvidence').mockResolvedValue(evidence)
  const el = document.createElement('div')
  const root = createRoot(el)
  try {
    await act(async () => root.render(<StageEvidence runId="run" />))
    // Owner Q8: a first measurement is not evidence of non-regression;
    // exit 0 does not override a failed comparable performance check.
    expect(el.textContent).toContain('Measured workload median: 250 ms')
    expect(el.textContent).not.toContain(i18n.t('evidence.exit', { code: reason === 'regressed' ? -2 : 0 }))
    expect(el.textContent).toContain(reason === 'regressed' ? 'Performance regressed' : 'First measured baseline; no prior comparison')
    if (reason === 'regressed') {
      expect(el.textContent).toContain('Comparable baseline: 100 ms')
      expect(el.textContent).toContain('Required: Performance')
      expect(el.textContent).not.toContain('Passed for current delivery')
    } else expect(el.textContent).not.toContain('Comparable baseline:')
    expect(el.textContent).not.toMatch(/FPS|Web Vitals|quality:performance/)
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})
