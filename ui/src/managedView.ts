// 托管的纯展示逻辑（ui-redesign 票 08，components D7 / D9）。
// 只把宿主给的码（ManagedRecord.result / action）映射成界面档位，不从 outcome 文本猜结果——
// result 由 core `result_of` 一张封闭表算出（test3/23 复审），这里只在 done 内再按动作分「已执行 /
// 已生效 / 已拒绝」三种外观；failed / incomplete 原样透出。
import type { ManagedRecord } from './gen/ManagedRecord'

export type RowChip = 'executed' | 'effective' | 'refused' | 'out_of_scope' | 'incomplete' | 'failed'
export type ChipTone = 'ok' | 'neutral' | 'warn' | 'err'

const REFUSALS = new Set(['deny', 'reject', 'refuse', 'abandon', 'end'])
const EXECUTIONS = new Set(['allow', 'install', 'publish', 'run_checks', 'new_attempt', 'retry', 'resume', 'resume_run', 'direct_use', 'continue'])

/** gap A5：宿主把「超出托管授权」分类为 out_of_scope（core `report::classify` 封闭表），优先于失败 / 未完成显示。 */
export function rowChip(record: Pick<ManagedRecord, 'result' | 'action'> & Partial<Pick<ManagedRecord, 'scope'>>): RowChip {
  if (record.scope === 'out_of_scope') return 'out_of_scope'
  if (record.result === 'failed') return 'failed'
  if (record.result === 'incomplete') return 'incomplete'
  if (REFUSALS.has(record.action)) return 'refused'
  return EXECUTIONS.has(record.action) ? 'executed' : 'effective'
}

export const CHIP_TONE: Record<RowChip, ChipTone> = {
  executed: 'ok', effective: 'ok', refused: 'neutral', out_of_scope: 'err', incomplete: 'warn', failed: 'err',
}

/** Trace timestamps are SQLite UTC (`YYYY-MM-DD HH:MM:SS`). */
export function parseTraceTime(at: string): Date | null {
  const parsed = new Date(`${at.replace(' ', 'T')}Z`)
  return Number.isNaN(parsed.getTime()) ? null : parsed
}

/** gap A2：托管时段分钟数（向下取整，至少 0）；ended 缺省 = 仍在托管，算到 now。 */
export function sessionMinutes(started: string | null, ended: string | null, now: number = Date.now()): number | null {
  const from = started ? parseTraceTime(started) : null
  if (!from) return null
  const to = ended ? parseTraceTime(ended)?.getTime() ?? now : now
  return Math.max(0, Math.floor((to - from.getTime()) / 60_000))
}
