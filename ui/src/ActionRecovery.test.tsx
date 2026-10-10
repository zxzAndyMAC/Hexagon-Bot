import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import './i18n'
import { api, type PendingQuestion } from './api'
import { PendingCard } from './components/PendingCards'
import { useUiStore } from './store'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
afterEach(() => vi.restoreAllMocks())

const card = (extra: Record<string, unknown> = {}): PendingQuestion => ({
  id: 'q9', kind: 'recovery', agent_id: 'a0', state: 'queued',
  payload: { sub: 'tool_outcome_unknown', action_id: 'act1', host_project_root: '/project',
    tool: 'fs_write', target: 'draft.txt', error_code: 'io', reconcile_supported: true, ...extra },
})

async function render(q: PendingQuestion) {
  useUiStore.setState({ pending: [q], toasts: [], modalScope: 'workbench' })
  const el = document.createElement('div'); const root = createRoot(el)
  await act(async () => root.render(<PendingCard q={q} top />))
  return { el, root }
}

it('names the real tool and target and reports every check, even with the same evidence', async () => {
  const report = { action_id: 'act1', verdict: 'still_unknown' as const, reason: 'evidence_inconclusive', check_count: 1, checked_at_ms: 0, continue_offered: false }
  const reconcile = vi.spyOn(api, 'reconcileToolAction')
    .mockResolvedValueOnce(report)
    .mockResolvedValueOnce({ ...report, check_count: 2 })
  const { el, root } = await render(card())
  try {
    expect(el.querySelector('[data-action-facts]')?.textContent).toContain('fs_write')
    expect(el.textContent).toContain('draft.txt')
    expect(el.querySelector('[data-capability]')?.textContent).toContain('read-only check')
    const check = el.querySelector('button[data-action="reconcileAction"]') as HTMLButtonElement
    await act(async () => check.click())
    expect(el.querySelector('[data-last-check]')?.textContent).toContain('Check 1')
    await act(async () => check.click())
    expect(el.querySelector('[data-last-check]')?.textContent).toContain('Check 2')
    expect(reconcile).toHaveBeenCalledTimes(2)
  } finally { await act(async () => root.unmount()) }
})

it('explains why abandon and new attempt are unavailable and offers continuation after abandon', async () => {
  const abandon = vi.spyOn(api, 'abandonToolAction').mockResolvedValue(undefined)
  const offer = vi.spyOn(api, 'offerContinueAfterUnknown').mockResolvedValue(true)
  const { el, root } = await render(card())
  try {
    const abandonButton = el.querySelector('button[data-action="abandonAction"]') as HTMLButtonElement
    expect(abandonButton.disabled).toBe(true)
    expect(abandonButton.title).toContain('Enter a reason first')
    expect(el.querySelector('[data-blocked]')?.textContent).toContain('Enter a reason first')
    const input = el.querySelector('input:not([type])') as HTMLInputElement
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      setter.call(input, 'checked by hand')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    const retry = el.querySelector('button[data-action="retryAction"]') as HTMLButtonElement
    expect(retry.title).toContain('Accept the duplicate-effect risk first')
    await act(async () => (el.querySelector('button[data-action="abandonAction"]') as HTMLButtonElement).click())
    expect(abandon).toHaveBeenCalledWith('act1', 'checked by hand', '/project')
    expect(offer).toHaveBeenCalledWith('act1')
  } finally { await act(async () => root.unmount()) }
})

it('does not offer a check the tool cannot do', async () => {
  const { el, root } = await render(card({ reconcile_supported: false }))
  try {
    expect(el.querySelector('button[data-action="reconcileAction"]')).toBeNull()
    expect(el.querySelector('[data-capability]')?.textContent).toContain('no read-only check')
  } finally { await act(async () => root.unmount()) }
})
