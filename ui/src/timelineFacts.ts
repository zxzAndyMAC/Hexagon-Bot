import type { TimelineFacts } from './gen/TimelineFacts'
import type { WorkbenchStatus } from './timelineModel'
import { parseTime } from './usage'

/** Issue15: current activity is a complete host projection, never a window scan. */
export function factOpenTurns(facts: TimelineFacts | null): Map<string | null, number | null> {
  return new Map((facts?.active_turns ?? []).map(turn => [turn.agent_id, parseTime(turn.started_at) || null]))
}
export function factOpenTurn(facts: TimelineFacts | null): { agentId: string | null; at: number | null } {
  const turn = facts?.active_turns.at(-1)
  return { agentId: turn?.agent_id ?? null, at: turn ? parseTime(turn.started_at) || null : null }
}
export function deriveWorkbenchStatusFromFacts(src: {
  stages: { stage: string; seq: number; state: string }[]
  team: { id: string; role: string }[]
  facts: TimelineFacts | null
  streams: Record<string, Record<number, string>>
  thinkings: Record<string, Record<number, string>>
  streamDone: Record<string, unknown>
}): WorkbenchStatus {
  const stage = src.stages.filter(value => value.state === 'active').sort((a, b) => b.seq - a.seq)[0]?.stage ?? null
  const live = [...new Set([...Object.keys(src.streams), ...Object.keys(src.thinkings)])].find(id =>
    src.streamDone[id] == null && [src.streams[id], src.thinkings[id]].some(buffer => buffer && Object.keys(buffer).length > 0))
  const agentId = live ?? factOpenTurn(src.facts).agentId
  return {
    stage,
    role: agentId ? src.team.find(member => member.id === agentId)?.role ?? agentId : null,
    tool: src.facts?.active_turns.find(turn => turn.agent_id === agentId)?.current_tool ?? null,
  }
}
