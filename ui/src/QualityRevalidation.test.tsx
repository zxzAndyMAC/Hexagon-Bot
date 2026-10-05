import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api, type PendingQuestion, type StageEvidence } from './api'
import { useUiStore } from './store'
import { PendingCard } from './components/PendingCards'
import { approveQuestion, handlePendingKey, kindTitleKey, rejectQuestion } from './decisions'
import { isMac } from './keymap'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const question: PendingQuestion = { id: 'revalidate', kind: 'stamp', agent_id: null, state: 'queued', payload: {
  sub: 'quality_revalidation', run_id: 'completed', project_root: '/project-one', final_acceptance: true,
} }
const evidence: StageEvidence = { run_id: 'completed', fingerprint: 'current-version', missing: [], checks: [], exceptions: [] }
const key = (name: string) => new KeyboardEvent('keydown', { key: name, metaKey: isMac, ctrlKey: !isMac, cancelable: true })

it('binds revalidation to displayed evidence, disables unavailable checks, and uses dedicated cancellation', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ modalScope: 'workbench', invalidate: async () => {}, artifacts: [] })
  const read = vi.spyOn(api, 'stageEvidence').mockResolvedValue(evidence)
  const confirm = vi.spyOn(api, 'confirmQualityRevalidation').mockResolvedValue({ action: 'pack_finished' })
  const cancel = vi.spyOn(api, 'cancelQualityRevalidation').mockResolvedValue()
  const reject = vi.spyOn(api, 'rejectStamp')
  const el = document.createElement('div')
  const root = createRoot(el)
  try {
    await act(async () => root.render(<PendingCard q={question} top />))
    const submit = el.querySelector<HTMLButtonElement>('button.primary')!
    expect(submit.disabled).toBe(false)
    expect(el.querySelector('select')).toBeNull()
    await act(async () => submit.click())
    expect(confirm).toHaveBeenCalledExactlyOnceWith('revalidate', 'current-version', '/project-one')
    read.mockResolvedValue({ ...evidence, fingerprint: 'changed-version', missing: ['check:quality:tests'] })
    await act(async () => { window.dispatchEvent(new Event('focus')) })
    expect(submit.disabled).toBe(true)
    await act(async () => submit.click())
    expect(confirm).toHaveBeenCalledTimes(1)
    await act(async () => [...el.querySelectorAll('button')].find(button => button.textContent === i18n.t('quality.revalidationCancel'))!.click())
    expect(cancel).toHaveBeenCalledWith('revalidate', '/project-one')
    expect(reject).not.toHaveBeenCalled()
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); useUiStore.setState(saved, true) }
})

it('ordinary dispatch cannot confirm a completed run or send cancellation to historical stage rejection', async () => {
  const saved = useUiStore.getState()
  useUiStore.setState({ modalScope: 'workbench', toasts: [] })
  const stamp = vi.spyOn(api, 'stamp')
  const reject = vi.spyOn(api, 'rejectStamp')
  const confirm = vi.spyOn(api, 'confirmQualityRevalidation')
  const cancel = vi.spyOn(api, 'cancelQualityRevalidation').mockResolvedValue()
  try {
    expect(await handlePendingKey(key('Enter'), [question])).toBe('blocked-final')
    expect(await handlePendingKey(key('Backspace'), [question])).toBe('blocked-final')
    await approveQuestion(question)
    await rejectQuestion(question)
    expect(kindTitleKey(question)).toBe('quality.revalidationTitle')
    expect(stamp).not.toHaveBeenCalled()
    expect(reject).not.toHaveBeenCalled()
    expect(confirm).not.toHaveBeenCalled()
    expect(cancel).toHaveBeenCalledExactlyOnceWith('revalidate', '/project-one')
  } finally { vi.restoreAllMocks(); useUiStore.setState(saved, true) }
})
