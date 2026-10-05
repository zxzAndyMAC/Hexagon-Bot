// 时间线密度变换与语义节点的纯函数层（ADR 0051）：过滤、tool call 折叠、节点刻度。
import type { TimelineItem } from './api'
import type { TimelineWindowPage } from './gen/TimelineWindowPage'
import { pairToolCalls } from './agentSteps'
import { parseTime } from './usage'
import type { IconName } from './components/Icon'

export type Filter = 'all' | 'messages' | 'decisions' | 'story'

export const DECISION_KINDS = new Set([
  'permission_asked', 'permission_allowed', 'permission_denied', 'permission_shape_remembered',
  'stamped', 'stamp_rejected', 'flag_submitted', 'flag_adjudicated', 'escalated',
  'proposal_queued', 'proposal_reviewed', 'proposal_stamped', 'proposal_rejected',
  'proposal_activated', 'proposal_rolled_back',
  'publish_requested', 'publish_confirmed', 'publish_rejected', 'publish_failed',
  'autonomy_changed', 'baseline_merged', 'stage_rewound',
  'pm_routed',
])

export const TOOL_KINDS = new Set(['tool_called', 'tool_result'])

/** Fullstack QA 2026-10-05: /pause and sleep deliberately close a turn.
 * Match only the recorded terminal outcome; a failure merely mentioning
 * Interrupted must remain an error. Crash recovery is also not a manual stop. */
export function isInterruptedTurn(item: TimelineItem): boolean {
  const outcome = item.event.payload.outcome ?? item.event.payload.error
  return item.event.kind === 'turn_failed' &&
    (outcome === 'Ok(Interrupted)' || outcome === 'Interrupted')
}

/** 票 02：自动盖章与负责人亲手盖章都是 `stamped`，靠 payload.by 区分。缺 by 的旧事件当人工。 */
export function stampedByAutonomy(payload: { by?: unknown } | null | undefined): boolean {
  return payload?.by === 'autonomy'
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
  timelineCaughtUp?: boolean
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

// beautiful-ui 票 04：升格导出——StreamFooter 首 token 等待位和 AgentTab
// 忙碌态都要「未收束回合」判定；at=turn_started 时间戳给 LoadingState 起表。
// exec-cards 票 03 修：并发回合交错——开窗按 agent 分槽，单栈会把
// 先开的回合误关（AgentTab 忙碌徽标漏报）。
export function openTurns(timeline: TimelineItem[], caughtUp = true): Map<string | null, number | null> {
  const m = new Map<string | null, number | null>()
  // Live acceptance 2026-10-01: a 500-row history page ended at a 588-minute-old
  // fs_read. Its completion lived on the next page, not in a running process.
  if (!caughtUp) return m
  for (const it of timeline) {
    // 2026-10-01 live acceptance #17: crash leftovers stayed busy after
    // team sleep or stage replacement. These boundaries retire old UI turns.
    if (it.event.kind === 'team_slept' || it.event.kind === 'stage_started') {
      m.clear()
    } else if (it.event.kind === 'turn_started') {
      m.delete(it.event.agent_id)
      m.set(it.event.agent_id, parseTime(it.event.created_at) || null)
    } else if (it.event.kind === 'turn_finished' || it.event.kind === 'turn_failed') {
      m.delete(it.event.agent_id)
    }
  }
  return m
}

export function openTurn(timeline: TimelineItem[], caughtUp = true): { agentId: string | null; at: number | null } {
  // 兼容单展示位（WaitingReply 只冒一个气泡）：取最后开的那个。
  let agentId: string | null = null
  let at: number | null = null
  for (const [a, t] of openTurns(timeline, caughtUp)) { agentId = a; at = t }
  return { agentId, at }
}

function openTurnAgent(timeline: TimelineItem[], caughtUp = true): string | null {
  return openTurn(timeline, caughtUp).agentId
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
  const agentId = liveAgent(src) ?? openTurnAgent(src.timeline, src.timelineCaughtUp)
  const role = agentId ? (src.team.find((m) => m.id === agentId)?.role ?? agentId) : null
  const tool = agentId && src.timelineCaughtUp !== false ? currentTool(src.timeline, agentId) : null
  return { stage, role, tool }
}

// EventRow 里有专门渲染的 kind；其余无消息事件落入 SystemRow，连续成片时折叠。
const SPECIAL_KINDS = new Set([
  'stage_started', 'return_summary', 'artifact_delivered', 'flag_submitted', 'flag_adjudicated',
  'pm_routed',
])

// 高危 system 子类词表（ui-audit 票 05 / ADR 0056-3）：
// invariant_violation / tool_breaker / context_denied 是需人注意的裁决信号，
// 折进 ≥3 折叠组会被扫读漏掉。取「提出组外」而非组标红计数——
// 组外零交互成本可见，红计数仍要求一次点击才看到是哪条违规。
// 与 trace.rs SYSTEM_SUBKINDS 登记表同步维护。
export const SYS_HIGH_RISK = new Set(['invariant_violation', 'tool_breaker', 'context_denied'])

// 2026-09-30 原生验收：供应商拒绝被折成系统事件计数，负责人误以为团队仍在工作。
const isHighRiskSys = (it: TimelineItem) =>
  it.event.kind === 'turn_failed' ||
  (it.event.kind === 'system' && SYS_HIGH_RISK.has(String(it.event.payload?.kind ?? '')))

const isSysItem = (it: TimelineItem) =>
  it.event.payload.kind !== 'agent_plan' &&
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
  // exec-cards 票 03：已收束回合的执行行收成一条摘要行（Cursor Worked-for-Xs
  // 借形）。folded 保留被折的原始行供展开还原；calls/secs/failed 是窗口统计。
  | { type: 'turnsummary'; idx: number; agentId: string | null; calls: number; secs: number | null; failed: boolean; folded: Row[]; partial?: boolean }

// exec-cards 票 03：回合窗口表——turn_started 开窗、turn_finished/failed 关窗
// （openTurn 同族逻辑，但这里要的是全部已收束窗口而非当前开着的那个）。
// 未收束窗口不进表 = 执行中过程永不折叠。
type TurnWindow = { startId: number; endId: number; agentId: string | null; failed: boolean; secs: number | null; calls?: number }

function settledTurns(timeline: TimelineItem[]): TurnWindow[] {
  const wins: TurnWindow[] = []
  // 按 agent 各自开窗——多 agent 并发回合在时间线里交错，单栈会把
  // A 的 finished 错关到 B 的窗上（串窗=摘要行张冠李戴）。
  const open = new Map<string | null, { startId: number; startAt: number }>()
  for (const it of timeline) {
    const ev = it.event
    if (ev.kind === 'turn_started') {
      open.set(ev.agent_id, { startId: ev.id, startAt: parseTime(ev.created_at) || 0 })
    } else if (ev.kind === 'turn_finished' || ev.kind === 'turn_failed') {
      const cur = open.get(ev.agent_id)
      if (!cur) continue
      open.delete(ev.agent_id)
      const endAt = parseTime(ev.created_at) || 0
      wins.push({
        startId: cur.startId,
        endId: ev.id,
        agentId: ev.agent_id,
        failed: ev.kind === 'turn_failed' && !isInterruptedTurn(it),
        secs: cur.startAt && endAt ? Math.max(0, (endAt - cur.startAt) / 1000) : null,
      })
    }
  }
  return wins
}

export function buildRows(timeline: TimelineItem[], filter: Filter, page?: Pick<TimelineWindowPage, 'boundary_pairs' | 'turn_windows' | 'chapter_base'>): Row[] {
  // Owner Q6 / 2026-10-02: pagination is a projection, not a new fold policy.
  // A result at the page boundary must still expose its off-page call evidence.
  if (page?.boundary_pairs.length && filter === 'all') {
    const visible = new Set(timeline.map(it => it.event.id))
    const calls = pairToolCalls([...timeline, ...page.boundary_pairs].sort((a, b) => a.event.id - b.event.id))
    const emitted = new Set<number>()
    timeline = timeline.flatMap(it => {
      if (it.event.kind !== 'tool_result') return [it]
      const call = calls.find(c => !visible.has(c.called.event.id) && !emitted.has(c.called.event.id) && (
        typeof it.event.payload.action_id === 'string'
          ? c.called.event.project_id === it.event.project_id && c.called.event.agent_id === it.event.agent_id
            && c.called.event.payload.action_id === it.event.payload.action_id
          : c.result?.event.id === it.event.id))
      if (!call) return [it]
      emitted.add(call.called.event.id)
      return [call.called, it]
    })
  }
  // 票 18（方向卡 3）：story 档——慢读复盘面。tool/sys 行全移除
  //（05 的高危子类豁免保留：invariant_violation 等仍可见），
  // turn_started 在消息流间插章节分隔。
  if (filter === 'story') {
    const rows: Row[] = []
    let n = page?.chapter_base ?? 0
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
    // Native 2026-10-01: permission/system events can split a call from its
    // result. Results are attached through the full call index; a result-only
    // group renders null and breaks Virtuoso's nonzero-height requirement.
    if (r.type === 'toolgroup' && !r.items.some((it) => it.event.kind === 'tool_called')) continue
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
  // exec-cards 票 03：回合摘要行——只 all 档生效（messages/decisions 本就筛掉
  // 执行行，story 走 chapter 先例）。窗口内 toolgroup/sysgroup 收成一条摘要
  // 行，落在最后被折行的槽位（Cursor 语法：执行块收在它与最终回复之间）；
  // 消息行与其余 item 原位保留——agent_message 是交付内容，折叠等于藏答案。
  if (filter !== 'all') return out
  const interruptedEnds = new Set(timeline.filter(isInterruptedTurn).map(item => item.event.id))
  const wins: TurnWindow[] = page ? page.turn_windows.flatMap(w => w.end_id == null ? [] : [{
    startId: w.start_id, endId: w.end_id, agentId: w.agent_id,
    failed: w.failed && !interruptedEnds.has(w.end_id),
    secs: w.ended_at && parseTime(w.started_at) && parseTime(w.ended_at)
      ? Math.max(0, (parseTime(w.ended_at) - parseTime(w.started_at)) / 1000) : null,
    calls: w.tool_call_count,
  }]) : settledTurns(timeline)
  if (!wins.length) return out
  const winOf = (r: Row): TurnWindow | null => {
    if (r.type !== 'toolgroup' && r.type !== 'sysgroup') return null
    // 行内全部事件严格落在窗口内才可折——turn_started/finished 被吸进
    // sysgroup 时该组跨界，保持原位不动。且不得带别家 agent 的事件
    // （并发回合 id 交错，否则把别家调用折进本窗摘要）；无归属的
    // ambient 系统行不排他。
    return wins.find(
      (w) => r.items.every(
        (x) => x.event.id > w.startId && x.event.id < w.endId
          && (x.event.agent_id == null || x.event.agent_id === w.agentId),
      ),
    ) ?? null
  }
  const final: Row[] = []
  const buckets = new Map<TurnWindow, { folded: Row[]; pos: number }>()
  for (const r of out) {
    const w = winOf(r)
    if (!w) {
      final.push(r)
      continue
    }
    const b = buckets.get(w) ?? { folded: [], pos: 0 }
    b.folded.push(r)
    b.pos = final.length
    buckets.set(w, b)
  }
  for (const [w, b] of [...buckets.entries()].sort((a, b2) => b2[1].pos - a[1].pos)) {
    const calls = b.folded.reduce(
      (n, r) => n + (r.type === 'toolgroup' ? r.items.filter((x) => x.event.kind === 'tool_called').length : 0),
      0,
    )
    final.splice(b.pos, 0, {
      type: 'turnsummary', idx: w.endId, agentId: w.agentId,
      calls: w.calls ?? calls, secs: w.secs, failed: w.failed, folded: b.folded, partial: w.calls != null && calls < w.calls,
    })
  }
  return final
}

export type NodeMark = { eventId?: number; rowIdx: number; icon: IconName; label: string; pending?: boolean }

export const NODE_ICONS: Record<string, IconName> = {
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

/** Source identities survive prepend regrouping and result-only boundary cards. */
export function rowContainsEvent(row: Row, id: number): boolean {
  if (row.type === 'chapter') return row.idx === id
  if (row.type === 'item') return row.item.event.id === id
  if (row.type === 'toolgroup' || row.type === 'sysgroup') return row.items.some(it => it.event.id === id)
  if (row.type === 'turnsummary') return row.folded.some(child => rowContainsEvent(child, id))
  return false
}

export function rowKey(row: Row): string { return `${row.type}:${row.idx}` }

/** Virtuoso counts rendered rows, not event records. Common rows keep their index. */
export function prependedRowCount(previous: Row[], next: Row[]): number {
  const positions = new Map(next.map((row, index) => [rowKey(row), index]))
  for (let index = 0; index < previous.length; index++) {
    const position = positions.get(rowKey(previous[index]))
    if (position != null) return position - index
  }
  return Math.max(0, next.length - previous.length)
}

/** Q6: prepending may change a group's first event/key, not its owner's choice. */
export function expandedAfterPrepend(previous: Row[], next: Row[], expanded: Set<number>): Set<number> {
  const openEvents = new Set<number>()
  const collect = (row: Row) => {
    if ((row.type === 'toolgroup' || row.type === 'sysgroup') && expanded.has(row.idx)) {
      for (const item of row.items) openEvents.add(item.event.id)
    }
    if (row.type === 'turnsummary') row.folded.forEach(collect)
  }
  previous.forEach(collect)
  const result = new Set(expanded)
  const retain = (row: Row) => {
    if ((row.type === 'toolgroup' || row.type === 'sysgroup') && row.items.some(item => openEvents.has(item.event.id))) result.add(row.idx)
    if (row.type === 'turnsummary') row.folded.forEach(retain)
  }
  next.forEach(retain)
  return result
}

export function rowAnchorEvent(row: Row): number {
  if (row.type === 'toolgroup' || row.type === 'sysgroup') return row.items[0].event.id
  if (row.type === 'turnsummary' && row.folded.length) return rowAnchorEvent(row.folded[0])
  return row.idx
}
