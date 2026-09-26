import { act, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import './i18n'
import { Timeline } from './components/Timeline'
import { useUiStore } from './store'
import type { TimelineItem } from './api'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent }: { data: unknown[]; itemContent: (i: number, row: unknown) => ReactNode }) =>
    <div>{data.map((row, i) => <div key={i}>{itemContent(i, row)}</div>)}</div>,
}))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it('unknown result after permission/turn events settles the original timeline action', async () => {
  const event = (id: number, kind: TimelineItem['event']['kind'], payload: Record<string, unknown>): TimelineItem => ({
    event: { id, kind, payload, agent_id: 'a1', project_id: 'p1', stage_run_id: null, created_at: '' }, message: null,
  })
  useUiStore.setState({ timeline: [
    event(1, 'tool_called', { tool: 'bash', action_id: 'A', input: { cmd: 'write-file' } }),
    event(2, 'permission_asked', {}),
    event(3, 'turn_failed', {}),
    event(4, 'tool_result', { tool: 'bash', action_id: 'A', ok: null, state: 'unknown' }),
  ], pending: [], team: [], streams: {}, thinkings: {}, streamDone: {} })
  const el = document.createElement('div')
  const root = createRoot(el)
  await act(async () => root.render(<Timeline />))
  const group = Array.from(el.querySelectorAll('.sysrow')).find((row) => row.textContent?.includes('1 tool calls'))!
  expect(group).toBeTruthy()
  await act(async () => group.dispatchEvent(new MouseEvent('click', { bubbles: true })))
  expect(el.textContent).toContain('Tool outcome unknown')
  expect(el.querySelector('.exec-card .spinner-ring')).toBeNull()
  await act(async () => root.unmount())
})

it('return summary exposes unresolved outcomes and exception history without expanding the card', async () => {
  useUiStore.setState({ timeline: [{ event: {
    id: 20, kind: 'return_summary', project_id: 'p1', agent_id: null, stage_run_id: null, created_at: '',
    payload: { since_event: 1, deliveries: [], pending_todos: [{ kind: 'permission', count: 1 }],
      attention: { unresolved_actions: 2, unknown_cost_records: 3, budget_stops: 1, exception_decisions: 1, policy_candidates: 1 } },
  }, message: null }], pending: [], team: [], streams: {}, thinkings: {}, streamDone: {} })
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<Timeline />))
    expect(el.textContent).toContain('Unknown action outcomes: 2')
    expect(el.textContent).toContain('Cost not yet known: 3')
    expect(el.textContent).toContain('Budget/capacity stops while away: 1')
    expect(el.textContent).toContain('Exception decisions while away (not passes): 1')
    expect(el.textContent).toContain('Policy candidates: 1')
  } finally { await act(async () => root.unmount()) }
})
