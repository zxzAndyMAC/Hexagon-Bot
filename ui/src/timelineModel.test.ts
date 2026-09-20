import { describe, expect, it } from 'vitest'
import { buildRows, nodeMarks } from './timelineModel'
import { severityOf } from './decisions'
import type { PendingQuestion, TimelineItem } from './api'

const ev = (id: number, kind: string, payload: Record<string, unknown> = {}, author?: string): TimelineItem => ({
  event: { id, kind, agent_id: author ?? null, stage_run_id: null, payload, created_at: '' },
  message: author ? { id, author, body: 'hi', tokens: [] } : null,
})

const q = (id: string, kind: string, payload: Record<string, unknown> = {}): PendingQuestion => ({
  id, kind, agent_id: null, payload, state: 'queued',
})

describe('buildRows', () => {
  const tl = [
    ev(1, 'stage_started', { stage: '实现' }),
    ev(2, 'tool_called', { tool: 'read' }),
    ev(3, 'tool_result'),
    ev(4, 'tool_called', { tool: 'write' }),
    ev(5, 'agent_message', {}, 'a1'),
    ev(6, 'permission_asked'),
  ]

  it('folds consecutive tool events into one group row', () => {
    const rows = buildRows(tl, 'all')
    expect(rows.map((r) => r.type)).toEqual(['item', 'toolgroup', 'item', 'item'])
    const g = rows[1]
    expect(g.type === 'toolgroup' && g.items.length).toBe(3)
  })

  it('filters messages and decisions', () => {
    const msgs = buildRows(tl, 'messages')
    expect(msgs).toHaveLength(1)
    expect(msgs[0].type === 'item' && msgs[0].item.event.kind).toBe('agent_message')
    const dec = buildRows(tl, 'decisions')
    expect(dec).toHaveLength(1)
    expect(dec[0].type === 'item' && dec[0].item.event.kind).toBe('permission_asked')
  })
})

describe('nodeMarks', () => {
  it('collects semantic nodes and pins pending first', () => {
    const tl = [
      ev(1, 'stage_started', { stage: '实现' }),
      ev(2, 'tool_called'),
      ev(3, 'artifact_delivered', { path: 'a/b.md' }),
      ev(4, 'stamped'),
      ev(5, 'flag_submitted', { flag_id: 'f1' }),
    ]
    const rows = buildRows(tl, 'all')
    const marks = nodeMarks(tl, rows, 2)
    expect(marks[0].pending).toBe(true)
    expect(marks[0].rowIdx).toBe(-1)
    expect(marks.map((m) => m.icon)).toEqual(['warn', 'stamp', 'artifact', 'check', 'flag'])
    expect(marks[1].label).toBe('实现')
    // tool_called 不进刻度
    expect(marks).toHaveLength(5)
  })
})

describe('severityOf', () => {
  it('orders publish > stage stamp > escalation > permission > proposal stamp', () => {
    const order = [
      q('p', 'publish'),
      q('s', 'stamp', { run_id: 'r1' }),
      q('e', 'escalation'),
      q('a', 'permission'),
      q('x', 'stamp', { proposal_id: 'pr1' }),
    ]
    const sorted = [...order].sort((a, b) => severityOf(a) - severityOf(b))
    expect(sorted.map((x) => x.id)).toEqual(['p', 's', 'e', 'a', 'x'])
    // stage stamp 与 proposal stamp 区分
    expect(severityOf(q('a', 'stamp', { run_id: 'r' }))).toBeLessThan(severityOf(q('b', 'stamp', { proposal_id: 'x' })))
  })
})

import { diffLines, parseUnifiedDiff, extractDiffBlock } from './diff'

describe('diffLines', () => {
  it('marks inserts and deletes around shared context', () => {
    const ops = diffLines(['a', 'b', 'c'], ['a', 'x', 'c'])
    expect(ops).toEqual([
      { type: 'eq', text: 'a' },
      { type: 'del', text: 'b' },
      { type: 'ins', text: 'x' },
      { type: 'eq', text: 'c' },
    ])
  })

  it('handles pure append', () => {
    const ops = diffLines(['a'], ['a', 'b'])
    expect(ops.filter((o) => o.type === 'ins')).toHaveLength(1)
    expect(ops.filter((o) => o.type === 'del')).toHaveLength(0)
  })
})

describe('parseUnifiedDiff / extractDiffBlock', () => {
  it('parses +/- prefixed lines', () => {
    const ops = parseUnifiedDiff('-old\n+new\n same')
    expect(ops.map((o) => o.type)).toEqual(['del', 'ins', 'eq'])
  })
  it('extracts fenced diff block from artifact body', () => {
    const body = '# 提案\n\n```diff\n-a\n+b\n```\n\nend'
    expect(extractDiffBlock(body)).toBe('-a\n+b')
    expect(extractDiffBlock('no block')).toBeNull()
  })
})
