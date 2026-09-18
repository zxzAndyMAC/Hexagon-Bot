// 时间线密度变换与语义节点的纯函数层（ADR 0051）：过滤、tool call 折叠、节点刻度。
import type { TimelineItem } from './api'
import type { IconName } from './components/Icon'

export type Filter = 'all' | 'messages' | 'decisions'

export const DECISION_KINDS = new Set([
  'permission_asked', 'permission_allowed', 'permission_denied', 'permission_shape_remembered',
  'stamped', 'stamp_rejected', 'flag_submitted', 'flag_adjudicated', 'escalated',
  'proposal_queued', 'proposal_reviewed', 'proposal_stamped', 'proposal_rejected',
  'proposal_activated', 'proposal_rolled_back',
  'publish_requested', 'publish_confirmed', 'publish_rejected', 'publish_failed',
  'autonomy_changed', 'baseline_merged', 'stage_rewound',
])

export const TOOL_KINDS = new Set(['tool_called', 'tool_result'])

export type Row =
  | { type: 'item'; item: TimelineItem; idx: number }
  | { type: 'toolgroup'; items: TimelineItem[]; idx: number }

export function buildRows(timeline: TimelineItem[], filter: Filter): Row[] {
  const vis = timeline.filter((it) =>
    filter === 'all' ? true
    : filter === 'messages' ? it.message != null
    : DECISION_KINDS.has(it.event.kind),
  )
  const rows: Row[] = []
  for (const it of vis) {
    const last = rows[rows.length - 1]
    if (TOOL_KINDS.has(it.event.kind) && last?.type === 'toolgroup') {
      last.items.push(it)
    } else if (TOOL_KINDS.has(it.event.kind)) {
      rows.push({ type: 'toolgroup', items: [it], idx: it.event.id })
    } else {
      rows.push({ type: 'item', item: it, idx: it.event.id })
    }
  }
  return rows
}

export type NodeMark = { rowIdx: number; icon: IconName; label: string; pending?: boolean }

const NODE_ICONS: Record<string, IconName> = {
  stage_started: 'stamp',
  stamped: 'check',
  artifact_delivered: 'artifact',
  flag_submitted: 'flag',
  flag_adjudicated: 'flag',
  escalated: 'warn',
}

export function nodeMarks(timeline: TimelineItem[], rows: Row[], pendingCount: number): NodeMark[] {
  const marks: NodeMark[] = []
  if (pendingCount > 0) marks.push({ rowIdx: -1, icon: 'warn', label: `${pendingCount}`, pending: true })
  const rowIndexByItem = new Map<number, number>()
  rows.forEach((r, i) => {
    if (r.type === 'item') rowIndexByItem.set(r.item.event.id, i)
  })
  for (const it of timeline) {
    const icon = NODE_ICONS[it.event.kind]
    const ri = rowIndexByItem.get(it.event.id)
    if (!icon || ri == null) continue
    const label = String(it.event.payload.stage ?? it.event.payload.path ?? it.event.payload.flag_id ?? it.event.kind)
    marks.push({ rowIdx: ri, icon, label })
  }
  return marks
}
