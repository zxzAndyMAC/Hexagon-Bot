import { describe, expect, it } from 'vitest'
import { buildRows, deriveWorkbenchStatus, nodeMarks, stampedByAutonomy, type StatusSources } from './timelineModel'
import { severityOf } from './decisions'
import type { EventKind, PendingQuestion, QueuedCard, TimelineItem } from './api'

const ev = (id: number, kind: EventKind, payload: Record<string, unknown> = {}, author?: string): TimelineItem => ({
  event: { id, project_id: 'p1', kind, agent_id: author ?? null, stage_run_id: null, payload, created_at: '' },
  message: author ? { id, author, body: 'hi', tokens: [], attachments: [], created_at: '', thinking: '' } : null,
})

const q = (id: string, kind: QueuedCard['kind'], payload: Record<string, unknown> = {}): PendingQuestion => ({
  id, kind, agent_id: null, payload, state: 'queued',
})

describe('stampedByAutonomy', () => {
  it('只把 by=autonomy 当成自动通过，缺 by 的旧事件当人工', () => {
    expect(stampedByAutonomy({ by: 'autonomy' })).toBe(true)
    expect(stampedByAutonomy({ by: 'owner' })).toBe(false)
    expect(stampedByAutonomy({})).toBe(false)
    expect(stampedByAutonomy(undefined)).toBe(false)
  })
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

  it('ui-audit 票 05：高危 system 子类提出折叠组外，始终可见', () => {
    const sys = (id: number, kind: string) => ev(id, 'system', { kind })
    const tl = [
      sys(1, 'provider_retry'),
      sys(2, 'known_world'),
      sys(3, 'invariant_violation'), // 高危——不得被吞
      sys(4, 'request_envelope'),
      sys(5, 'tool_breaker'),        // 高危——不得被吞
      sys(6, 'steering_injected'),
      sys(7, 'context_denied'),      // 高危——不得被吞
      sys(8, 'judge_verdict'),
    ]
    const rows = buildRows(tl, 'all')
    // 高危行把折叠切成若干段；每段 <3 不折，高危行本身永为 item
    const high = [3, 5, 7]
    for (const id of high) {
      const r = rows.find((x) => x.type === 'item' && x.item.event.id === id)
      expect(r, `event ${id} must render standalone`).toBeTruthy()
    }
    // 低危段 1-2 两条不够 3 → 各自独立行；8 单条独立
    expect(rows.filter((r) => r.type === 'sysgroup')).toHaveLength(0)
    expect(rows).toHaveLength(8)
  })

  it('ui-audit 票 05：低危 system 连续 ≥3 仍折叠', () => {
    const sys = (id: number, kind: string) => ev(id, 'system', { kind })
    const tl = [sys(1, 'provider_retry'), sys(2, 'known_world'), sys(3, 'request_envelope'), sys(4, 'steering_injected')]
    const rows = buildRows(tl, 'all')
    expect(rows).toHaveLength(1)
    expect(rows[0].type).toBe('sysgroup')
    expect(rows[0].type === 'sysgroup' && rows[0].items).toHaveLength(4)
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

describe('story 档（ui-audit 票 18 / 方向卡 3）', () => {
  const mk = (id: number, kind: string, msg = false): TimelineItem => ({
    event: { id, project_id: 'p1', kind, agent_id: 'a1', stage_run_id: null, payload: kind === 'turn_started' ? { stage: 'build' } : {}, created_at: '' },
    message: msg ? { id, author: 'a1', body: 'hi', tokens: [], attachments: [], created_at: '', thinking: '' } : null,
  }) as unknown as TimelineItem

  it('toolgroup/sysgroup 全移除；高危子类豁免可见；turn_started 成章', () => {
    const rows = buildRows([
      mk(1, 'turn_started'),
      mk(2, 'agent_message', true),
      mk(3, 'tool_called'),
      mk(4, 'tool_result'),
      mk(5, 'system'),                       // 普通 sys → 移除
      mk(6, 'system'),                       // 同上（凑组）
      mk(7, 'system'),
      { ...mk(8, 'system'), event: { ...mk(8, 'system').event, payload: { kind: 'invariant_violation' } } }, // 高危豁免
      mk(9, 'turn_started'),
      mk(10, 'agent_message', true),
    ], 'story')
    expect(rows.every((r) => r.type !== 'toolgroup' && r.type !== 'sysgroup')).toBe(true)
    const chapters = rows.filter((r) => r.type === 'chapter')
    expect(chapters.length).toBe(2)
    expect(chapters[0]).toMatchObject({ n: 1, agentId: 'a1', stage: 'build' })
    const items = rows.filter((r) => r.type === 'item').map((r) => (r as { item: TimelineItem }).item.event.id)
    expect(items).toEqual([2, 8, 10]) // 消息 + 高危豁免；普通 sys 与 tool 全不见
  })
})

describe('deriveWorkbenchStatus（hands-free 票 06）', () => {
  const base = (): StatusSources => ({
    stages: [
      { stage: '规格', seq: 1, state: 'done' },
      { stage: '实现', seq: 2, state: 'active' },
    ],
    team: [{ id: 'a1', role: '后端开发' }],
    timeline: [],
    streams: {},
    thinkings: {},
    streamDone: {},
  })

  it('阶段、在跑角色和当前工具来自工作台，不读模型自述的计划', () => {
    const src = base()
    src.streams = { a1: { 1: '正在写' } }
    src.timeline = [
      ev(1, 'turn_started', {}, 'a1'),
      ev(2, 'agent_message', {}, 'a1'),
      ev(3, 'tool_called', { tool: 'fs_read' }, 'a1'),
    ]
    src.timeline[1].message = {
      id: 2, author: 'a1', tokens: [], attachments: [], created_at: '',
      body: '计划：阶段是发布，下一步用 fs_write',
      thinking: '',
    }
    const st = deriveWorkbenchStatus(src)
    expect(st).toEqual({ stage: '实现', role: '后端开发', tool: 'fs_read' })
  })

  it('工具结果或权限询问之后不再显示该工具', () => {
    const src = base()
    src.timeline = [
      ev(1, 'turn_started', {}, 'a1'),
      ev(2, 'tool_called', { tool: 'fs_read' }, 'a1'),
      ev(3, 'tool_result', {}, 'a1'),
    ]
    expect(deriveWorkbenchStatus(src).tool).toBeNull()
    src.timeline = [
      ev(1, 'turn_started', {}, 'a1'),
      ev(2, 'tool_called', { tool: 'fs_write' }, 'a1'),
      ev(3, 'permission_asked', { tool: 'fs_write' }, 'a1'),
    ]
    expect(deriveWorkbenchStatus(src).tool).toBeNull()
  })

  it('没有进行中的回合时不编造角色和工具', () => {
    const src = base()
    src.timeline = [
      ev(1, 'turn_started', {}, 'a1'),
      ev(2, 'turn_finished', {}, 'a1'),
    ]
    expect(deriveWorkbenchStatus(src)).toEqual({ stage: '实现', role: null, tool: null })
  })

  it('多个 active 阶段取序号最大的', () => {
    const src = base()
    src.stages.push({ stage: '复审', seq: 3, state: 'active' })
    expect(deriveWorkbenchStatus(src).stage).toBe('复审')
  })
})
