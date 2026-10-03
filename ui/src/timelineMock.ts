// Browser-development projection only. Production queries remain the typed core
// seam; these fixtures preserve full history so UI exports/paging can be exercised.
import type { TimelineItem } from './gen/TimelineItem'
import type { TimelineWindowRequest } from './gen/TimelineWindowRequest'
import type { TimelineWindowPage } from './gen/TimelineWindowPage'
import type { TimelineFactsRequest } from './gen/TimelineFactsRequest'
import type { TimelineFacts } from './gen/TimelineFacts'
import type { TimelineNodesRequest } from './gen/TimelineNodesRequest'
import type { TimelineNodesPage } from './gen/TimelineNodesPage'
import type { TimelineTurnWindow } from './gen/TimelineTurnWindow'
import type { ContextSnapshot } from './gen/ContextSnapshot'
import { DECISION_KINDS, SYS_HIGH_RISK } from './timelineModel'
import { pairToolCalls } from './agentSteps'

function steered(all: TimelineItem[], wanted: number[]) {
  return all.filter(item => item.event.kind === 'system' && item.event.payload.kind === 'steering_injected')
    .map(item => Number(item.event.payload.msg_id)).filter(id => wanted.includes(id))
}
export function mockTimelineWindow(all: TimelineItem[], request: TimelineWindowRequest): TimelineWindowPage {
  const filtered = all.filter(item => (!request.agent_id || item.event.agent_id === request.agent_id) && (
    request.filter === 'all' || request.filter === 'messages' && !!item.message ||
    request.filter === 'decisions' && DECISION_KINDS.has(item.event.kind) || request.filter === 'story' && (
      !!item.message || DECISION_KINDS.has(item.event.kind) || ['turn_started', 'turn_failed'].includes(item.event.kind) ||
      item.event.kind === 'system' && SYS_HIGH_RISK.has(String(item.event.payload.kind)))))
  const n = Math.max(1, Math.min(500, request.limit))
  const cursor = request.cursor
  let items: TimelineItem[]
  if (cursor.kind === 'latest') items = filtered.slice(-n)
  else if (cursor.kind === 'before') items = filtered.filter(item => item.event.id < cursor.event_id).slice(-n)
  else if (cursor.kind === 'after') items = filtered.filter(item => item.event.id > cursor.event_id).slice(0, n)
  else {
    const index = filtered.findIndex(item => item.event.id === cursor.event_id)
    if (index < 0) throw new Error('Timeline target unavailable')
    const start = Math.max(0, Math.min(index - Math.floor(n / 2), filtered.length - n))
    items = filtered.slice(start, start + n)
  }
  const ids = new Set(items.map(item => item.event.id))
  const boundary = new Map<number, TimelineItem>()
  for (const call of pairToolCalls(all)) if (ids.has(call.called.event.id) || call.result && ids.has(call.result.event.id)) {
    if (!ids.has(call.called.event.id)) boundary.set(call.called.event.id, call.called)
    if (call.result && !ids.has(call.result.event.id)) boundary.set(call.result.event.id, call.result)
  }
  const open = new Map<string | null, TimelineTurnWindow>()
  const turns: TimelineTurnWindow[] = []
  for (const item of all) {
    const event = item.event
    if (event.kind === 'turn_started') {
      const turn = { start_id: event.id, end_id: null, agent_id: event.agent_id, started_at: event.created_at, ended_at: null, failed: false, tool_call_count: 0 }
      open.set(event.agent_id, turn); turns.push(turn)
    } else {
      const turn = open.get(event.agent_id)
      if (turn && event.kind === 'tool_called') turn.tool_call_count++
      if (turn && ['turn_finished', 'turn_failed'].includes(event.kind)) {
        turn.end_id = event.id; turn.ended_at = event.created_at; turn.failed = event.kind === 'turn_failed'; open.delete(event.agent_id)
      }
    }
  }
  const first = items[0]?.event.id ?? 0, last = items.at(-1)?.event.id ?? 0
  return { project_root: request.expected_project_root, items, boundary_pairs: [...boundary.values()],
    turn_windows: turns.filter(turn => items.some(item => item.event.agent_id === turn.agent_id && item.event.id >= turn.start_id && (turn.end_id == null || item.event.id <= turn.end_id))),
    chapter_base: all.filter(item => item.event.kind === 'turn_started' && item.event.id < first && (!request.agent_id || item.event.agent_id === request.agent_id)).length,
    has_before: filtered.some(item => item.event.id < first), has_after: items.length > 0 && filtered.some(item => item.event.id > last),
    watermark: all.at(-1)?.event.id ?? 0, target_event_id: cursor.kind === 'around' ? cursor.event_id : null,
    steered_message_ids: steered(all, items.flatMap(item => item.message ? [item.message.id] : [])),
  }
}
export function mockTimelineFacts(all: TimelineItem[], request: TimelineFactsRequest): TimelineFacts {
  const facts: TimelineFacts = { project_root: request.expected_project_root, latest_event_id: all.at(-1)?.event.id ?? 0,
    active_turns: [], latest_contexts: [], latest_agent_messages: [], latest_plans: [], settled_tool_streams: [],
    approval_mode_revision: 0, latest_turn_start_id: null, steered_message_ids: steered(all, request.watched_message_ids) }
  for (const { event, message } of all) {
    const agent = event.agent_id
    if (['team_slept', 'stage_started'].includes(event.kind)) facts.active_turns = []
    if (event.kind === 'turn_started') {
      facts.latest_turn_start_id = event.id
      facts.active_turns = [...facts.active_turns.filter(turn => turn.agent_id !== agent), { agent_id: agent, start_event_id: event.id, started_at: event.created_at, current_tool: null }]
    }
    if (['turn_finished', 'turn_failed'].includes(event.kind)) facts.active_turns = facts.active_turns.filter(turn => turn.agent_id !== agent)
    const turn = facts.active_turns.find(value => value.agent_id === agent)
    if (turn && event.kind === 'tool_called') turn.current_tool = String(event.payload.tool ?? '') || null
    if (turn && ['tool_result', 'permission_asked', 'permission_denied'].includes(event.kind)) turn.current_tool = null
    if (message) facts.latest_agent_messages = [...facts.latest_agent_messages.filter(value => value.agent_id !== message.author), { agent_id: message.author, event_id: event.id }]
    if (event.kind === 'system' && event.payload.kind === 'approval_mode_changed') facts.approval_mode_revision = event.id
    if (agent && event.kind === 'system' && event.payload.kind === 'agent_plan') facts.latest_plans = [...facts.latest_plans.filter(value => value.agent_id !== agent), { agent_id: agent, event_id: event.id, text: String(event.payload.text ?? '') }]
    if (agent && event.kind === 'system' && event.payload.kind === 'request_envelope' && event.payload.context != null) {
      const value = event.payload.context as ContextSnapshot
      const valid = [value.used_tokens, value.window_tokens, value.compact_at_tokens].every(n => typeof n === 'number' && Number.isFinite(n) && n >= 0) && value.window_tokens > 0
      facts.latest_contexts = [...facts.latest_contexts.filter(value => value.agent_id !== agent), { agent_id: agent, event_id: event.id, created_at: event.created_at, context: valid ? value : null }]
    }
  }
  const pairs = pairToolCalls(all)
  facts.settled_tool_streams = request.watched_tool_streams.filter(key => pairs.some(call => call.result && call.called.event.agent_id === key.agent_id && String(call.called.event.payload.seq) === key.seq))
  return facts
}
export function mockTimelineNodes(all: TimelineItem[], request: TimelineNodesRequest): TimelineNodesPage {
  const kinds = ['stage_started','stamped','artifact_delivered','flag_submitted','flag_adjudicated','escalated','check_overridden','install_completed']
  return { project_root: request.expected_project_root, watermark: all.at(-1)?.event.id ?? 0,
    nodes: all.filter(item => item.event.id > (request.after_event_id ?? 0) && kinds.includes(item.event.kind)).map(({event}) => ({ event_id: event.id, kind: event.kind, label: String(event.payload.stage ?? event.payload.path ?? event.payload.flag_id ?? event.kind) })) }
}
