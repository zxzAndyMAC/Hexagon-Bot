// 归来摘要的纯逻辑（ui-redesign 票 05，components C12、interaction §7）。
// 载荷形状 = core autonomy.rs ReturnSummary；本模块只做投影，不发 IPC。
// 「需要你处理」直接取 store.pending 的当下事实，而不是载荷里的离开期间计数：
// 列表首行带 ⌘↵，必须和待决坞的顶卡是同一张卡（R-02：排序沿用 SEVERITY）。
import type { PendingQuestion, TimelineItem } from './api'
import type { ReturnSummary } from './gen/ReturnSummary'
import { severityOf } from './decisions'
import { parseTime } from './usage'

export type AttentionTone = 'irreversible' | 'authorize' | 'unknown'

export type AttentionRow = { question: PendingQuestion; tone: AttentionTone }

/** 不可逆（红 rocket）→ 需授权（橙 shield）→ 未知结果（灰 alert）只决定色调；
 *  顺序完全跟 severityOf，保证首行 = 坞顶卡（⌘↵ 作用对象）。 */
export function attentionTone(q: PendingQuestion): AttentionTone {
  if (q.payload.sub === 'tool_outcome_unknown') return 'unknown'
  if (q.kind === 'publish') return 'irreversible'
  return 'authorize'
}

export function attentionRows(pending: readonly PendingQuestion[]): AttentionRow[] {
  return pending
    .map((question, order) => ({ question, order }))
    // 稳定排序：同严重度保持队列序，与 usePendingKeys 的 sort 结果一致
    .sort((a, b) => severityOf(a.question) - severityOf(b.question) || a.order - b.order)
    .map(({ question }) => ({ question, tone: attentionTone(question) }))
}

/** C12 禁用态（补设计，票 04 收口票 05 的缺口）：处理掉的「需要你处理」行划除并转 ink-3，保留 30 秒可读后移除。 */
export const HANDLED_TTL_MS = 30_000

export type ShownRow = AttentionRow & { handledAt: number | null }

/** 上一帧显示的行 + 当下待决 → 这一帧的行：仍待决的按严重度在前（首行 = 坞顶卡），
 *  刚离开 pending 的记下离开时刻排在后面，超过 ttl 的丢弃。纯函数，时间由调用方给。 */
export function withHandled(prev: readonly ShownRow[], rows: readonly AttentionRow[], now: number, ttl = HANDLED_TTL_MS): ShownRow[] {
  const live = new Set(rows.map((r) => r.question.id))
  const handled = prev
    .filter((r) => !live.has(r.question.id))
    .map((r) => (r.handledAt == null ? { ...r, handledAt: now } : r))
    .filter((r) => now - (r.handledAt ?? now) < ttl)
  return [...rows.map((r) => ({ ...r, handledAt: null })), ...handled]
}

/** 载荷里可能缺字段（旧轨迹、测试夹具）——缺的一律按 0，不臆造。 */
export function readSummary(payload: Record<string, unknown>): ReturnSummary {
  const group = <T extends Record<string, number>>(key: string, keys: (keyof T)[]): T => {
    const raw = (payload[key] ?? {}) as Record<string, unknown>
    return Object.fromEntries(keys.map((k) => [k, Number(raw[k as string] ?? 0) || 0])) as T
  }
  const list = <T,>(key: string): T[] => (Array.isArray(payload[key]) ? (payload[key] as T[]) : [])
  return {
    since_event: Number(payload.since_event ?? 0) || 0,
    deliveries: list('deliveries'),
    reviews: group('reviews', ['passed', 'rejected']),
    flags: group('flags', ['submitted', 'adjudicated', 'escalated']),
    permissions: group('permissions', ['asked', 'allowed', 'denied']),
    stages: group('stages', ['finished', 'skipped', 'rewound']),
    pending_todos: list('pending_todos'),
    attention: group('attention', ['unresolved_actions', 'unknown_cost_records', 'budget_stops', 'exception_decisions', 'policy_candidates']),
  }
}

export const ATTENTION_KEYS = ['unresolved_actions', 'unknown_cost_records', 'budget_stops', 'exception_decisions', 'policy_candidates'] as const

/** 离开区间：离开时刻 = since_event 的时间（不在已载窗口里则未知），归来 = 摘要事件时间。 */
export function awayRange(summary: TimelineItem, items: readonly TimelineItem[]): { from: string | null; to: string; minutes: number | null } {
  const since = Number(summary.event.payload.since_event ?? 0)
  const left = since > 0 ? items.find((it) => it.event.id === since) : undefined
  const to = summary.event.created_at
  if (!left) return { from: null, to, minutes: null }
  const ms = parseTime(to) - parseTime(left.event.created_at)
  return { from: left.event.created_at, to, minutes: Number.isFinite(ms) ? Math.max(0, Math.round(ms / 60000)) : null }
}

/** 建议下一步（补设计：core 不产出建议，UI 由摘要事实推导，只填入输入条不发送）。
 *  key 是 i18n 键尾，params 是插值；最多 3 条，按「先处理返工 → 再看交付 → 再推进」。 */
export type Suggestion = { key: 'rejected' | 'escalated' | 'delivery' | 'stage' | 'keepGoing'; params: Record<string, string | number> }

export function suggestions(s: ReturnSummary): Suggestion[] {
  const out: Suggestion[] = []
  if (s.reviews.rejected > 0) out.push({ key: 'rejected', params: { count: s.reviews.rejected } })
  if (s.flags.escalated > 0) out.push({ key: 'escalated', params: { count: s.flags.escalated } })
  const firstPath = s.deliveries.find((d) => d.path)?.path
  if (firstPath) out.push({ key: 'delivery', params: { path: firstPath } })
  if (s.stages.finished > 0) out.push({ key: 'stage', params: {} })
  if (!out.length) out.push({ key: 'keepGoing', params: {} })
  return out.slice(0, 3)
}

/** 最新一条归来摘要只在负责人还没开口时钉顶：负责人一旦发出指令，摘要的使命完成，
 *  回到时间线里作为一行标记。返回 null = 不钉。 */
export function pinnedSummary(items: readonly TimelineItem[]): TimelineItem | null {
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i]
    if (it.event.kind === 'return_summary') return it
    if (it.event.kind === 'owner_message' || it.message?.author === 'owner') return null
  }
  return null
}

/** 首屏引导（C13）：负责人还没发过任何指令。只在拿到完整历史（没有更早一页）时判定，
 *  否则窗口外的旧指令会让老项目误显示问候。 */
export function isFreshProject(items: readonly TimelineItem[], fullHistory: boolean): boolean {
  return fullHistory && !items.some((it) => it.event.kind === 'owner_message' || it.message?.author === 'owner')
}
