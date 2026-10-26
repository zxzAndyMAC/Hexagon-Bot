// 会话胶囊的显示条件与能力合并（components C12a「改为头部胶囊」，owner-followups-1015 #11）。
// 纯函数：输入会话摘要（状态、能力、预约）与读取错误，输出「隐藏 / 显示 + 色调 + 最严重项」。
// 健康 = 运行中且能力全 healthy 且无预约 → 隐藏，时间线与归来摘要不再被常驻卡挤占（1440×900 下「需要你处理」曾被裁）。
import type { CapabilityHealth } from './gen/CapabilityHealth'
import type { CapabilityKind } from './gen/CapabilityKind'
import type { CapabilityStatus } from './gen/CapabilityStatus'
import type { SessionStatus } from './gen/SessionStatus'
import type { SessionSummary } from './gen/SessionSummary'

export type PillTone = 'err' | 'warn' | 'info' | 'ok' | 'neutral'

export type PillReason =
  | { kind: 'error' }
  | { kind: 'status'; status: SessionStatus }
  | { kind: 'capability'; group: CapabilityGroup }
  | { kind: 'expired' }
  | { kind: 'scheduled' }

export type SessionPillView = { kind: 'hidden' } | { kind: 'shown'; tone: PillTone; reason: PillReason }

export type CapabilityGroup = {
  kind: CapabilityKind
  healthy: number
  total: number
  worst: CapabilityHealth
  items: CapabilityStatus[]
}

const TONE_RANK: Record<PillTone, number> = { err: 4, warn: 3, info: 2, ok: 1, neutral: 0 }

// 能力合并时「最差」的次序。unknown 排在 healthy 之上、degraded 之下：没探测过不等于坏了。
const HEALTH_RANK: Record<CapabilityHealth, number> = { unavailable: 4, reconnecting: 3, degraded: 2, unknown: 1, healthy: 0 }

/** 状态 → 胶囊色调；null = 这个状态本身不算异常，不让胶囊出现。
 * waiting_user 不算：它是「坞里有卡等负责人」的常态，坞自己会出现，胶囊再出一次只是重复（规范 C12a 补设计，#11）。 */
const STATUS_TONE: Record<SessionStatus, PillTone | null> = {
  created: null, running: null, waiting_user: null,
  paused: 'info', reconnecting: 'warn', completed: 'ok', failed: 'err',
}

/** 能力健康 → 胶囊色调；null = 不算异常。
 * unknown 不算：provider / computer_use 未探测时常年是 unknown，算进来胶囊就永远不消失（#11）。 */
const HEALTH_TONE: Record<CapabilityHealth, PillTone | null> = {
  healthy: null, unknown: null, degraded: 'warn', reconnecting: 'warn', unavailable: 'err',
}

/** 能力按 kind 合并（「MCP 3/6 正常」），色取最差；保留首次出现的 kind 次序。 */
export function groupCapabilities(caps: readonly CapabilityStatus[]): CapabilityGroup[] {
  const groups = new Map<CapabilityKind, CapabilityGroup>()
  for (const cap of caps) {
    const prev = groups.get(cap.kind)
    const items = [...(prev?.items ?? []), cap]
    const worst = prev && HEALTH_RANK[prev.worst] >= HEALTH_RANK[cap.health] ? prev.worst : cap.health
    groups.set(cap.kind, {
      kind: cap.kind,
      healthy: (prev?.healthy ?? 0) + (cap.health === 'healthy' ? 1 : 0),
      total: items.length,
      worst,
      items,
    })
  }
  return [...groups.values()]
}

/** 显示条件（规范 C12a）。同档时次序 = 读取错误 > 会话状态 > 能力 > 预约。 */
export function sessionPillView(summary: SessionSummary | null, error: string | null): SessionPillView {
  const session = summary?.session ?? null
  if (!session) return error ? { kind: 'shown', tone: 'err', reason: { kind: 'error' } } : { kind: 'hidden' }
  const candidates: { tone: PillTone; reason: PillReason }[] = []
  const statusTone = STATUS_TONE[session.status]
  if (statusTone) candidates.push({ tone: statusTone, reason: { kind: 'status', status: session.status } })
  for (const group of groupCapabilities(summary?.capabilities ?? [])) {
    const tone = HEALTH_TONE[group.worst]
    if (tone) candidates.push({ tone, reason: { kind: 'capability', group } })
  }
  const triggers = summary?.triggers ?? []
  if (triggers.some((t) => t.state === 'expired')) candidates.push({ tone: 'warn', reason: { kind: 'expired' } })
  if (triggers.some((t) => t.state === 'scheduled')) candidates.push({ tone: 'neutral', reason: { kind: 'scheduled' } })
  if (candidates.length === 0) return { kind: 'hidden' }
  // 稳定取最高档：同档保留先入者（push 次序即上面的同档次序）
  const top = candidates.reduce((best, c) => (TONE_RANK[c.tone] > TONE_RANK[best.tone] ? c : best))
  return { kind: 'shown', ...top }
}

/** 转入 failed / reconnecting 时播报一次 + 单次脉冲；首次读到（prev 为 undefined）不算转入。 */
export function alertTransition(prev: SessionStatus | null | undefined, next: SessionStatus | null): 'failed' | 'reconnecting' | null {
  if (prev === undefined || prev === next) return null
  return next === 'failed' || next === 'reconnecting' ? next : null
}

/** 「稍后继续…」此刻做不了的原因（i18n key）；null = 可约。⋯ 菜单 / ⌘K / 键位 / 胶囊弹层共用一处判定（C12a #11）：
 * 没有会话、会话已结束（completed / failed）、已有一次预约（一次只挂一个，取消在胶囊弹层里）。 */
export function laterBlockedReason(summary: SessionSummary | null): string | null {
  const session = summary?.session ?? null
  if (!session) return 'session.laterNoSession'
  if (session.status === 'completed' || session.status === 'failed') return 'session.laterEnded'
  if (summary?.triggers.some((t) => t.state === 'scheduled')) return 'session.laterBooked'
  return null
}
