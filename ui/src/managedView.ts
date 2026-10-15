// 托管的纯展示逻辑（ui-redesign 票 08，components D7 / D9）。
// 只把宿主给的码（ManagedRecord.result / action）映射成界面档位，不从 outcome 文本猜结果——
// result 由 core `result_of` 一张封闭表算出（test3/23 复审），这里只在 done 内再按动作分「已执行 /
// 已生效 / 已拒绝」三种外观；failed / incomplete 原样透出。
import type { TimelineItem } from './api'
import type { ManagedRecord } from './gen/ManagedRecord'

export type RowChip = 'executed' | 'effective' | 'refused' | 'incomplete' | 'failed'
export type ChipTone = 'ok' | 'neutral' | 'warn' | 'err'

const REFUSALS = new Set(['deny', 'reject', 'refuse', 'abandon', 'end'])
const EXECUTIONS = new Set(['allow', 'install', 'publish', 'run_checks', 'new_attempt', 'retry', 'resume', 'resume_run', 'direct_use', 'continue'])

export function rowChip(record: Pick<ManagedRecord, 'result' | 'action'>): RowChip {
  if (record.result === 'failed') return 'failed'
  if (record.result === 'incomplete') return 'incomplete'
  if (REFUSALS.has(record.action)) return 'refused'
  return EXECUTIONS.has(record.action) ? 'executed' : 'effective'
}

export const CHIP_TONE: Record<RowChip, ChipTone> = {
  executed: 'ok', effective: 'ok', refused: 'neutral', incomplete: 'warn', failed: 'err',
}

/** Trace timestamps are SQLite UTC (`YYYY-MM-DD HH:MM:SS`). */
export function parseTraceTime(at: string): Date | null {
  const parsed = new Date(`${at.replace(' ', 'T')}Z`)
  return Number.isNaN(parsed.getTime()) ? null : parsed
}

/** When the current delegation was turned on, if that fact is in the loaded
 * timeline window (`system.delegation_changed`, enabled). Unknown → null:
 * the pill and report then omit the duration instead of guessing. */
export function delegationSince(timeline: TimelineItem[]): Date | null {
  for (let i = timeline.length - 1; i >= 0; i--) {
    const ev = timeline[i].event
    if (ev.kind !== 'system' || ev.payload?.kind !== 'delegation_changed') continue
    return ev.payload.enabled === true ? parseTraceTime(ev.created_at) : null
  }
  return null
}

export function minutesSince(since: Date, now = Date.now()): number {
  return Math.max(0, Math.floor((now - since.getTime()) / 60000))
}

/** The newest managed decision row for one card, for the strip's「查看」. */
export function latestDecisionEvent(timeline: TimelineItem[], cardId: string): number | null {
  for (let i = timeline.length - 1; i >= 0; i--) {
    const ev = timeline[i].event
    if (ev.kind === 'system' && ev.payload?.kind === 'managed_decision' && ev.payload.card_id === cardId) return ev.id
  }
  return null
}
