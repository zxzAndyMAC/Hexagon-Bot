import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { PendingCard } from './components/PendingCards'
import { StageEvidence } from './components/StageEvidence'
import { approveQuestion, handlePendingKey, rejectQuestion } from './decisions'
import { isMac } from './keymap'
import type { StageEvidence as Evidence } from './gen/StageEvidence'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const question: PendingQuestion = { id: 'exception', kind: 'stamp', agent_id: null, state: 'queued', payload: {
  sub: 'acceptance_exception', run_id: 'run', fingerprint: 'v1:A',
} }
const requirement = { kind: 'check' as const, run_id: 'run', cmd: 'verify' }
const evidence: Evidence = { run_id: 'run', fingerprint: 'v1:A', missing: ['check:verify'],
  checks: [{ run_id: 'run', stage: 'build', cmd: 'verify', exit_code: 1, state: 'failed', event_id: 5 }],
  exceptions: [{ requirement, label: 'build · verify', accepted: false }],
}
const key = (special = false) => new KeyboardEvent('keydown', {
  key: 'Enter', metaKey: isMac, ctrlKey: !isMac, altKey: special, shiftKey: special, cancelable: true,
})

it('requires explicit selection and reason, blocks modal shortcuts, and invalidates changed delivery', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ modalScope: 'workbench', invalidate: async () => {} })
  const read = vi.spyOn(api, 'stageEvidence').mockResolvedValue(evidence)
  const accept = vi.spyOn(api, 'acceptDeliveryException').mockResolvedValue({ event_id: 8 })
  const el = document.createElement('div')
  document.body.append(el)
  const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCard q={question} top />))
    const submit = el.querySelector<HTMLButtonElement>('button.primary')!
    expect(submit.disabled).toBe(true)
    await act(async () => el.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click())
    expect(submit.disabled).toBe(true)
    await act(async () => {
      const input = el.querySelector('textarea')!
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, 'known fixture failure')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(submit.disabled).toBe(false)
    useUiStore.setState({ modalScope: 'settings' })
    await act(async () => { window.dispatchEvent(key(true)) })
    expect(accept).not.toHaveBeenCalled()
    useUiStore.setState({ modalScope: 'workbench' })
    await act(async () => { window.dispatchEvent(key(true)) })
    expect(accept).toHaveBeenCalledExactlyOnceWith('exception', 'v1:A', [requirement], 'known fixture failure')
    read.mockResolvedValue({ ...evidence, fingerprint: 'v1:B' })
    await act(async () => { window.dispatchEvent(new Event('focus')) })
    expect(submit.disabled).toBe(true)
    expect(el.textContent).toContain('Delivery changed')
  } finally {
    await act(async () => root.unmount())
    el.remove(); vi.restoreAllMocks(); useUiStore.setState(saved, true)
  }
})

it('never dispatches ordinary approval or cancellation to the stage stamp', async () => {
  const saved = useUiStore.getState()
  useUiStore.setState({ modalScope: 'workbench', invalidate: async () => {} })
  const stamp = vi.spyOn(api, 'stamp')
  const reject = vi.spyOn(api, 'rejectStamp')
  const cancel = vi.spyOn(api, 'cancelAcceptanceException').mockResolvedValue()
  try {
    expect(await handlePendingKey(key(), [question])).toBe('blocked-exception')
    await approveQuestion(question)
    await rejectQuestion(question)
    expect(stamp).not.toHaveBeenCalled()
    expect(reject).not.toHaveBeenCalled()
    expect(cancel).toHaveBeenCalledWith('exception')
  } finally { vi.restoreAllMocks(); useUiStore.setState(saved, true) }
})

it('shows exception acceptance alongside the original failed result', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'stageEvidence').mockResolvedValue({ ...evidence, missing: [], exceptions: [{ ...evidence.exceptions[0], accepted: true }] })
  const el = document.createElement('div')
  const root = createRoot(el)
  try {
    await act(async () => root.render(<StageEvidence />))
    expect(el.textContent).toContain('Accepted by exception')
    expect(el.textContent).toContain('Failed')
    expect(el.textContent).toContain('Recorded exit code: 1')
    expect(el.textContent).not.toContain('Passed for current delivery')
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})
