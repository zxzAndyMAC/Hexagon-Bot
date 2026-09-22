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

/** 票 02：自动盖章与负责人亲手盖章都是 `stamped`，靠 payload.by 区分。缺 by 的旧事件当人工。 */
export function stampedByAutonomy(payload: { by?: unknown } | null | undefined): boolean {
  return payload?.by === 'autonomy'
}

// hands-free 票 06：过长思考收成一行。按字符，不靠 scrollHeight
// （happy-dom 测不出布局）。短思考原样，不假装要折叠。
export const THINKING_COLLAPSE_AT = 72

export function thinkingCollapsed(text: string): { long: boolean; line: string } {
  const flat = text.replace(/\s+/g, ' ').trim()
  const chars = [...flat]
  const long = /[\r\n]/.test(text) || chars.length > THINKING_COLLAPSE_AT
  const line = long ? `${chars.slice(0, THINKING_COLLAPSE_AT).join('')}…` : flat
  return { long, line }
}

// hands-free 票 06：状态行只读工作台状态（阶段行、未收束回合、tool_called）。
// 否决：从模型回复里抽「下一步 / 阶段」——那是自述计划，不是正在发生的事。
export type WorkbenchStatus = {
  stage: string | null
  role: string | null
  tool: string | null
}

export type StatusSources = {
  stages: { stage: string; seq: number; state: string }[]
  team: { id: string; role: string }[]
  timeline: TimelineItem[]
  streams: Record<string, Record<number, string>>
  thinkings: Record<string, Record<number, string>>
  streamDone: Record<string, unknown>
}

function currentStage(stages: StatusSources['stages']): string | null {
  let best: { stage: string; seq: number } | null = null
  for (const s of stages) {
    if (s.state !== 'active') continue
    if (!best || s.seq >= best.seq) best = { stage: s.stage, seq: s.seq }
  }
  return best?.stage ?? null
}

function bufOpen(m?: Record<number, string>): boolean {
  return !!m && Object.keys(m).length > 0
}

/** 还在收增量、且回合未标完结的角色。工作台一轮一个回合，取第一个即可。 */
function liveAgent(src: StatusSources): string | null {
  const ids = new Set([...Object.keys(src.streams), ...Object.keys(src.thinkings)])
  for (const id of ids) {
    if (src.streamDone[id] != null) continue
    if (bufOpen(src.streams[id]) || bufOpen(src.thinkings[id])) return id
  }
  return null
}

function openTurnAgent(timeline: TimelineItem[]): string | null {
  let open: string | null = null
  for (const it of timeline) {
    if (it.event.kind === 'turn_started') open = it.event.agent_id
    else if (it.event.kind === 'turn_finished' || it.event.kind === 'turn_failed') open = null
  }
  return open
}

function currentTool(timeline: TimelineItem[], agentId: string): string | null {
  let tool: string | null = null
  let inTurn = false
  for (const it of timeline) {
    const ev = it.event
    if (ev.kind === 'turn_started') {
      inTurn = ev.agent_id === agentId
      if (inTurn) tool = null
      continue
    }
    if (ev.kind === 'turn_finished' || ev.kind === 'turn_failed') {
      if (ev.agent_id == null || ev.agent_id === agentId) {
        inTurn = false
        tool = null
      }
      continue
    }
    if (!inTurn || ev.agent_id !== agentId) continue
    if (ev.kind === 'tool_called') {
      const name = String(ev.payload?.tool ?? '')
      tool = name || null
    } else if (
      ev.kind === 'tool_result' ||
      ev.kind === 'permission_asked' ||
      ev.kind === 'permission_denied'
    ) {
      tool = null
    }
  }
  return tool
}

export function deriveWorkbenchStatus(src: StatusSources): WorkbenchStatus {
  const stage = currentStage(src.stages)
  const agentId = liveAgent(src) ?? openTurnAgent(src.timeline)
  const role = agentId ? (src.team.find((m) => m.id === agentId)?.role ?? agentId) : null
  const tool = agentId ? currentTool(src.timeline, agentId) : null
  return { stage, role, tool }
}

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
