import { expect, it, vi } from 'vitest'
import { api, type PendingQuestion } from './api'
import { approveQuestion, rejectQuestion } from './decisions'
import { useUiStore } from './store'

it('cannot approve or reject a visual direction through generic card adjudication', async () => {
  const saved = useUiStore.getState()
  const toast = vi.fn()
  useUiStore.setState({ pushToast: toast })
  const adjudicate = vi.spyOn(api, 'adjudicateFlag')
  const choose = vi.spyOn(api, 'chooseDesignDirection')
  const q: PendingQuestion = { id: 'design', kind: 'escalation', agent_id: 'ux', state: 'queued', payload: { sub: 'design_direction', revision: 1 } }
  try {
    // Owner Q5: a generic approval must never substitute for selecting a
    // particular versioned preview, including keyboard-triggered approval.
    await approveQuestion(q)
    await rejectQuestion(q)
    expect(adjudicate).not.toHaveBeenCalled()
    expect(choose).not.toHaveBeenCalled()
    expect(toast).toHaveBeenCalledTimes(2)
  } finally { vi.restoreAllMocks(); useUiStore.setState(saved, true) }
})
