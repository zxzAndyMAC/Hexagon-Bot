// 时间线密度变换与语义节点的纯函数层（ADR 0051）：过滤、tool call 折叠、节点刻度。
import type { TimelineItem } from './api'
import type { IconName } from './components/Icon'

export type Filter = 'all' | 'messages' | 'decisions' | 'story'

export const DECISION_KINDS = new Set([
  'permission_asked', 'permission_allowed', 'permission_denied', 'permission_shape_remembered',
  'stamped', 'stamp_rejected', 'flag_submitted', 'flag_adjudicated', 'escalated',
  'proposal_queued', 'proposal_reviewed', 'proposal_stamped', 'proposal_rejected',
  'proposal_activated', 'proposal_rolled_back',
  'publish_requested', 'publish_confirmed', 'publish_rejected', 'publish_failed',
  'autonomy_changed', 'baseline_merged', 'stage_rewound',
])

export const TOOL_KINDS = new Set(['tool_called', 'tool_result'])

// EventRow 里有专门渲染的 kind；其余无消息事件落入 SystemRow，连续成片时折叠。
const SPECIAL_KINDS = new Set([
  'stage_started', 'return_summary', 'artifact_delivered', 'flag_submitted', 'flag_adjudicated',
])

// 高危 system 子类词表（ui-audit 票 05 / ADR 0056-3）：
// invariant_violation / tool_breaker / context_denied 是需人注意的裁决信号，
// 折进 ≥3 折叠组会被扫读漏掉。取「提出组外」而非组标红计数——
// 组外零交互成本可见，红计数仍要求一次点击才看到是哪条违规。
// 与 trace.rs SYSTEM_SUBKINDS 登记表同步维护。
export const SYS_HIGH_RISK = new Set(['invariant_violation', 'tool_breaker', 'context_denied'])

const isHighRiskSys = (it: TimelineItem) =>
  it.event.kind === 'system' && SYS_HIGH_RISK.has(String(it.event.payload?.kind ?? ''))

const isSysItem = (it: TimelineItem) =>
  !TOOL_KINDS.has(it.event.kind) &&
  !SPECIAL_KINDS.has(it.event.kind) &&
  it.message == null &&
  !isHighRiskSys(it)

const SYS_GROUP_MIN = 3

export type Row =
  | { type: 'item'; item: TimelineItem; idx: number }
  | { type: 'toolgroup'; items: TimelineItem[]; idx: number }
  | { type: 'sysgroup'; items: TimelineItem[]; idx: number }
  // 票 18（方向卡 3）：story 档的回合章节分隔——turn_started 事件升格为章节行。
  | { type: 'chapter'; idx: number; n: number; agentId: string | null; stage: string }

export function buildRows(timeline: TimelineItem[], filter: Filter): Row[] {
  // 票 18（方向卡 3）：story 档——慢读复盘面。tool/sys 行全移除
  //（05 的高危子类豁免保留：invariant_violation 等仍可见），
  // turn_started 在消息流间插章节分隔。
  if (filter === 'story') {
    const rows: Row[] = []
    let n = 0
    for (const it of timeline) {
      if (it.event.kind === 'turn_started') {
        n++
        rows.push({
          type: 'chapter',
          idx: it.event.id,
          n,
          agentId: it.event.agent_id,
          stage: String(it.event.payload?.stage ?? ''),
        })
        continue
      }
      if (it.message != null || isHighRiskSys(it) || DECISION_KINDS.has(it.event.kind)) {
        rows.push({ type: 'item', item: it, idx: it.event.id })
      }
    }
    return rows
  }
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
  const out: Row[] = []
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i]
    if (r.type === 'item' && isSysItem(r.item)) {
      let j = i
      while (j < rows.length) {
        const x = rows[j]
        if (x.type !== 'item' || !isSysItem(x.item)) break
        j++
      }
      if (j - i >= SYS_GROUP_MIN) {
        out.push({ type: 'sysgroup', items: rows.slice(i, j).map((x) => (x as { item: TimelineItem }).item), idx: r.item.event.id })
        i = j - 1
        continue
      }
    }
    out.push(r)
  }
  return out
}

export type NodeMark = { rowIdx: number; icon: IconName; label: string; pending?: boolean }

const NODE_ICONS: Record<string, IconName> = {
  stage_started: 'stamp',
  stamped: 'check',
  artifact_delivered: 'artifact',
  flag_submitted: 'flag',
  flag_adjudicated: 'flag',
  escalated: 'warn',
  check_overridden: 'warn',
  install_completed: 'install',
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
