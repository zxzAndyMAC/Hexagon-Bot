import { expect, it } from 'vitest'
import { buildRows, expandedAfterPrepend, prependedRowCount, rowContainsEvent, rowKey } from './timelineModel'
import type { TimelineItem } from './api'
import type { TimelineWindowPage } from './gen/TimelineWindowPage'

const event = (id: number, kind: TimelineItem['event']['kind'], payload: Record<string, unknown> = {}): TimelineItem => ({
  event: { id, kind, payload, agent_id: 'a1', project_id: 'p1', stage_run_id: null, created_at: '' }, message: null,
})
const metadata = (overrides: Partial<Pick<TimelineWindowPage, 'boundary_pairs' | 'turn_windows' | 'chapter_base'>> = {}) => ({
  boundary_pairs: [], turn_windows: [], chapter_base: 0, ...overrides,
})

it('a result-only newest page keeps its off-page call evidence and source identity', () => {
  // Issue15: discarding a result-only group hid the newest durable outcome.
  const called = event(2, 'tool_called', { action_id: 'a', tool: 'computer_observe' })
  const result = event(100, 'tool_result', { action_id: 'a', ok: true })
  const rows = buildRows([result], 'all', metadata({ boundary_pairs: [called] }))
  expect(rows).toHaveLength(1)
  expect(rows[0].type).toBe('toolgroup')
  expect(rowContainsEvent(rows[0], 100)).toBe(true)
  expect(rowContainsEvent(rows[0], 2)).toBe(true)
  const complete = buildRows([called, result], 'all', metadata({ boundary_pairs: [called] }))
  expect(complete).toHaveLength(1)
  expect(complete[0].type === 'toolgroup' && complete[0].items.filter(item => item.event.kind === 'tool_called')).toHaveLength(1)
})

it('a partial long turn keeps settled folding, complete counts and elapsed time', () => {
  const rows = buildRows([event(50, 'tool_called', { action_id: 'x' }), event(51, 'tool_result', { action_id: 'x', ok: true })], 'all', metadata({
    turn_windows: [{ start_id: 1, end_id: 1000, agent_id: 'a1', started_at: '2026-10-02T00:00:00Z', ended_at: '2026-10-02T00:01:30Z', failed: false, tool_call_count: 250 }],
  }))
  expect(rows).toHaveLength(1)
  expect(rows[0]).toMatchObject({ type: 'turnsummary', idx: 1000, calls: 250, secs: 90 })
  expect(rowContainsEvent(rows[0], 50)).toBe(true)
  expect(rowContainsEvent(rows[0], 51)).toBe(true)
  expect(rowKey(rows[0])).toBe('turnsummary:1000')
})

it('an active turn is never folded using a historical completion from another agent', () => {
  const rows = buildRows([event(50, 'tool_called')], 'all', metadata({
    turn_windows: [{ start_id: 1, end_id: null, agent_id: 'a1', started_at: '', ended_at: null, failed: false, tool_call_count: 1 },
      { start_id: 1, end_id: 100, agent_id: 'a2', started_at: '', ended_at: '', failed: false, tool_call_count: 2 }],
  }))
  expect(rows[0].type).toBe('toolgroup')
})

it('story chapter numbering remains global when a previous page is loaded', () => {
  const later = buildRows([event(70, 'turn_started')], 'story', metadata({ chapter_base: 42 }))
  expect(later[0]).toMatchObject({ type: 'chapter', idx: 70, n: 43 })
  const merged = buildRows([event(60, 'turn_started'), event(70, 'turn_started')], 'story', metadata({ chapter_base: 41 }))
  expect(merged[1]).toMatchObject({ type: 'chapter', idx: 70, n: 43 })
})

it('prepend adjusts virtual row indices by surviving rows, not event count', () => {
  const previous = buildRows([event(9, 'stage_started'), event(10, 'flag_submitted')], 'all')
  const next = buildRows([event(1, 'system'), event(2, 'system'), event(3, 'system'), event(9, 'stage_started'), event(10, 'flag_submitted')], 'all')
  expect(prependedRowCount(previous, next)).toBe(1)
  expect(rowKey(previous[0])).toBe(rowKey(next[1]))
})

it('a merged group can still locate its prior source event when its row key changes', () => {
  const previous = buildRows([event(5, 'tool_called'), event(6, 'tool_result')], 'all')
  const next = buildRows([event(3, 'tool_called'), event(4, 'tool_result'), event(5, 'tool_called'), event(6, 'tool_result')], 'all')
  expect(rowKey(previous[0])).not.toBe(rowKey(next[0]))
  expect(rowContainsEvent(next[0], 5)).toBe(true)
})

it('a newer off-page receipt does not make an older visible result lose its call card', () => {
  const called = event(2, 'tool_called', { action_id: 'a', tool: 'computer_action' })
  const rows = buildRows([event(100, 'tool_result', { action_id: 'a', ok: null }), event(120, 'tool_result', { action_id: 'a', ok: null })], 'all', metadata({
    boundary_pairs: [called, event(200, 'tool_result', { action_id: 'a', ok: true })],
  }))
  expect(rows).toHaveLength(1)
  expect(rows[0].type === 'toolgroup' && rows[0].items.filter(item => item.event.kind === 'tool_called')).toHaveLength(1)
  expect(rowContainsEvent(rows[0], 100)).toBe(true)
})

it('prepend preserves an expanded first group when its key changes to an earlier event', () => {
  const previous = buildRows([event(100, 'tool_called'), event(101, 'tool_result')], 'all')
  const next = buildRows([event(90, 'tool_called'), event(91, 'tool_result'), event(100, 'tool_called'), event(101, 'tool_result')], 'all')
  const open = expandedAfterPrepend(previous, next, new Set([100]))
  expect(open.has(90)).toBe(true)
  expect(rowContainsEvent(next[0], 100)).toBe(true)
  expect(expandedAfterPrepend(previous, next, new Set()).has(90)).toBe(false)
})

it('a synthetic turn summary never steals the identity of the real finish event', () => {
  const rows = buildRows([event(1, 'turn_started'), event(2, 'tool_called'), event(3, 'tool_result'), event(4, 'artifact_delivered'), event(5, 'turn_finished')], 'all')
  expect(rows.filter(row => rowContainsEvent(row, 5))).toHaveLength(1)
  expect(rows.find(row => rowContainsEvent(row, 5))?.type).toBe('item')
  expect(rows.find(row => row.type === 'turnsummary')).toBeTruthy()
})
