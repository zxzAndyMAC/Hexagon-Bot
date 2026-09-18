import type { UsagePoint, UsageRow } from './api'

/** 账本单位：成本以 millicents 计，上限以 cents 计。1 ¥ = 100 cents = 100_000 mc。 */
export const MC_PER_YUAN = 100000
export const MC_PER_CENT = 1000
export const CENTS_PER_YUAN = 100

export const fmtYuan = (mc?: number | null) =>
  mc == null ? '—' : `¥${(mc / MC_PER_YUAN).toFixed(2)}`

export const centsToMc = (cents: number) => cents * MC_PER_CENT

export const capReached = (spentMc: number, limitCents: number | null | undefined) =>
  limitCents != null && spentMc >= centsToMc(limitCents)

/** 分解行：滤掉 _total 汇总行。 */
export const breakdownRows = (rows: UsageRow[]) => rows.filter((r) => !r._total)

/** 按键聚合成本（模型分解用），按成本降序。 */
export function groupCost(rows: UsageRow[], key: (r: UsageRow) => string): [string, number][] {
  const out = new Map<string, number>()
  for (const r of rows) out.set(key(r), (out.get(key(r)) ?? 0) + (r.cost_mc ?? 0))
  return [...out.entries()].sort((a, b) => b[1] - a[1])
}

/** 曲线最大值（归一化用），空序列回 1 防除零。 */
export const seriesMax = (points: UsagePoint[]) => Math.max(1, ...points.map((p) => p.cost_mc))

/** 用户输入 ¥ → cents（API 入参）；非法/非正 → null = 清除上限。 */
export const parseLimitInput = (raw: string): number | null => {
  const yuan = parseFloat(raw)
  return Number.isFinite(yuan) && yuan > 0 ? Math.round(yuan * CENTS_PER_YUAN) : null
}
