// 用量入口三档（components B4，usage#quick；interaction §10）：<80% 只有图标 · ≥80% 橙色百分比 · 触顶红色「上限」。
// 数据只取 `usage_summary` 的项目总计（store.usageTotal），不新增 IPC。形状 + 文字升级，不只靠颜色。
import type { UsageTotal } from './api'
import { capReached, centsToMc } from './usage'

/** ≥ 这个百分比进 warn 档（usage#quick「≥80%」）。 */
export const USAGE_WARN_PCT = 80

export type UsageTier = {
  /** unknown = 首次读取前或读取失败：只显示图标，不显示过期百分比（B4 加载 / 错误行）。 */
  tier: 'unknown' | 'ok' | 'warn' | 'cap'
  /** 未设上限时为 null（只显示图标，可读名不带百分比）。 */
  pct: number | null
}

/** 「提高上限」：打开设置「用量」分区（App 监听 hexagon:open-settings-section，票 12）。 */
export const openUsageSettings = () =>
  window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: 'usage' }))

export function usageTier(total: UsageTotal | null): UsageTier {
  if (!total) return { tier: 'unknown', pct: null }
  if (total.limit_cents == null) return { tier: 'ok', pct: null }
  if (capReached(total.spent_mc, total.limit_cents)) return { tier: 'cap', pct: 100 }
  const limit = centsToMc(total.limit_cents)
  // 上限为 0 时 capReached 已为真；这里 limit > 0。向下取整：79.6% 不提前报警。
  const pct = Math.floor((total.spent_mc / limit) * 100)
  return { tier: pct >= USAGE_WARN_PCT ? 'warn' : 'ok', pct }
}
