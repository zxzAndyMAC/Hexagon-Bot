import { describe, expect, it } from 'vitest'
import type { PendingQuestion, TimelineItem } from './api'
import { attentionRows, awayRange, pinnedSummary, readSummary, suggestions } from './returnSummaryModel'

const q = (id: string, kind: string, payload: Record<string, unknown> = {}): PendingQuestion =>
  ({ id, kind, agent_id: 'a1', payload, state: 'queued' }) as unknown as PendingQuestion

const ev = (id: number, kind: string, payload: Record<string, unknown> = {}, created_at = '', author?: string): TimelineItem => ({
  event: { id, kind, payload, agent_id: null, project_id: 'p', stage_run_id: null, created_at } as TimelineItem['event'],
  message: author ? ({ id, author, body: 'x' } as TimelineItem['message']) : null,
})

describe('归来摘要「需要你处理」', () => {
  it('按 SEVERITY 排序（R-02），首行就是坞顶卡；色调只看卡种', () => {
    const rows = attentionRows([
      q('perm', 'permission'),
      q('unk', 'recovery', { sub: 'tool_outcome_unknown' }),
      q('pub', 'publish'),
      q('stamp', 'stamp'),
    ])
    expect(rows.map((r) => r.question.id)).toEqual(['unk', 'pub', 'stamp', 'perm'])
    expect(rows.map((r) => r.tone)).toEqual(['unknown', 'irreversible', 'authorize', 'authorize'])
  })

  it('同严重度保持队列序', () => {
    expect(attentionRows([q('p1', 'permission'), q('p2', 'permission')]).map((r) => r.question.id)).toEqual(['p1', 'p2'])
  })
})

describe('建议下一步', () => {
  it('先返工、再交付、再推进，最多 3 条', () => {
    const s = readSummary({ reviews: { passed: 3, rejected: 1 }, flags: { escalated: 2 }, deliveries: [{ path: 'src/a.ts', kind: 'code' }], stages: { finished: 1 } })
    expect(suggestions(s).map((x) => x.key)).toEqual(['rejected', 'escalated', 'delivery'])
  })

  it('什么都没发生时给一条「继续推进」，不编造事实', () => {
    expect(suggestions(readSummary({})).map((x) => x.key)).toEqual(['keepGoing'])
  })
})

describe('钉顶与区间', () => {
  it('负责人在摘要之后开口，摘要不再钉顶', () => {
    const summary = ev(5, 'return_summary', { since_event: 1 })
    expect(pinnedSummary([ev(1, 'system'), summary])).toBe(summary)
    expect(pinnedSummary([summary, ev(6, 'owner_message', {}, '', 'owner')])).toBeNull()
  })

  it('离开时刻取 since_event；不在窗口里就不报分钟数', () => {
    const summary = ev(5, 'return_summary', { since_event: 1 }, '2026-10-13 14:02:00')
    expect(awayRange(summary, [ev(1, 'system', {}, '2026-10-13 13:40:00'), summary]).minutes).toBe(22)
    expect(awayRange(summary, [summary]).minutes).toBeNull()
  })
})
