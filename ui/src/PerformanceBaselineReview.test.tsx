import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { api } from './api'
import i18n from './i18n'
import { PerformanceBaselineReview } from './components/PerformanceBaselineReview'
import { useUiStore } from './store'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
it('requires an owner reason and binds confirmation to the displayed actual measurement', async () => {
  await i18n.changeLanguage('en')
  const confirm = vi.spyOn(api, 'confirmPerformanceBaseline').mockResolvedValue({ event_id: 15, measurement_event_id: 12 })
  const el = document.createElement('div')
  const root = createRoot(el)
  try {
    await act(async () => root.render(<PerformanceBaselineReview measurement={12} expected="version:one" />))
    await act(async () => el.querySelector<HTMLButtonElement>('button')!.click())
    expect(el.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true)
    const input = el.querySelector('input')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '  changed benchmark fixture  ')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => el.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })))
    expect(confirm).toHaveBeenCalledWith(12, 'version:one', 'changed benchmark fixture')
    expect(el.textContent).toContain(i18n.t('quality.baselineConfirmed'))
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})

it('keeps a confirmation notice when refreshed evidence remounts the review', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'confirmPerformanceBaseline').mockResolvedValue({ event_id: 15, measurement_event_id: 12 })
  const el = document.createElement('div')
  const root = createRoot(el)
  const before = useUiStore.getState()
  useUiStore.setState({ toasts: [], invalidate: async () => {
    // Owner QA 2026-10-06 / issue 07: confirmation changes the delivery
    // fingerprint, so StageEvidence gives this review a new key. Its local
    // success state disappears even though the backend accepted the baseline.
    root.render(<PerformanceBaselineReview key="12:version:two" measurement={12} expected="version:two" />)
  } })
  try {
    await act(async () => root.render(<PerformanceBaselineReview key="12:version:one" measurement={12} expected="version:one" />))
    await act(async () => el.querySelector<HTMLButtonElement>('button')!.click())
    const input = el.querySelector('input')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'changed workload')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => el.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })))
    expect(el.querySelector('form')).toBeNull()
    expect(useUiStore.getState().toasts).toContainEqual(expect.objectContaining({ text: i18n.t('quality.baselineConfirmed'), tone: 'ok' }))
  } finally {
    await act(async () => root.unmount())
    useUiStore.setState({ invalidate: before.invalidate, toasts: before.toasts })
    vi.restoreAllMocks()
  }
})
