import type { UsageBucket, UsageRow } from './api'

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

/** token 数动态单位：999 → "999"，84_400 → "84.4K"，3_530_000_000 → "3.53B"。 */
export const fmtTok = (n: number): string => {
  const units: [number, string][] = [[1e9, 'B'], [1e6, 'M'], [1e3, 'K']]
  for (const [div, u] of units) {
    if (n >= div) {
      const s = n / div
      // 三位有效数字，去小数尾零（1.00 → 1，84.40 → 84.4）
      const str = s.toPrecision(3).replace(/\.?0+$/, '')
      return `${str}${u}`
    }
  }
  return String(Math.round(n))
}

export const tokensOf = (b: UsageBucket) =>
  b.prompt_tokens + b.completion_tokens + b.tool_output_tokens

/** 序列整形：去重排序的 bucket 轴 + 每个 agent 一条 (bucket→tokens) 序列。 */
export function perAgentSeries(rows: UsageBucket[]): {
  labels: string[]
  series: { agentId: string; points: number[] }[]
} {
  const labels = [...new Set(rows.map((r) => r.bucket))].sort()
  const idx = new Map(labels.map((l, i) => [l, i]))
  const byAgent = new Map<string, number[]>()
  for (const r of rows) {
    const id = r.agent_id ?? '—'
    if (!byAgent.has(id)) byAgent.set(id, new Array(labels.length).fill(0))
    byAgent.get(id)![idx.get(r.bucket)!] += tokensOf(r)
  }
  return { labels, series: [...byAgent.entries()].map(([agentId, points]) => ({ agentId, points })) }
}

/** 三类 token 各自一条序列（跨 agent 聚合）：prompt / completion / tool_output。 */
export function tokenTypeSeries(rows: UsageBucket[]): { labels: string[]; series: number[][] } {
  const labels = [...new Set(rows.map((r) => r.bucket))].sort()
  const idx = new Map(labels.map((l, i) => [l, i]))
  const out = [new Array(labels.length).fill(0), new Array(labels.length).fill(0), new Array(labels.length).fill(0)]
  for (const r of rows) {
    const i = idx.get(r.bucket)!
    out[0][i] += r.prompt_tokens
    out[1][i] += r.completion_tokens
    out[2][i] += r.tool_output_tokens
  }
  return { labels, series: out }
}

/** 按键聚合 token（榜单/分解用），按 token 降序。 */
export function groupTokens(
  rows: { prompt_tokens?: number; completion_tokens?: number; tool_output_tokens?: number; agent_id?: string | null; model?: string | null; stage?: string | null }[],
  key: (r: (typeof rows)[number]) => string,
): [string, number][] {
  const out = new Map<string, number>()
  for (const r of rows) {
    const tok = (r.prompt_tokens ?? 0) + (r.completion_tokens ?? 0) + (r.tool_output_tokens ?? 0)
    out.set(key(r), (out.get(key(r)) ?? 0) + tok)
  }
  return [...out.entries()].sort((a, b) => b[1] - a[1])
}

/** 曲线最大值（归一化用），空序列回 1 防除零。 */
export const seriesMax = (points: UsageBucket[]) => Math.max(1, ...points.map((p) => p.cost_mc))

/** 用户输入 ¥ → cents（API 入参）；非法/非正 → null = 清除上限。 */
export const parseLimitInput = (raw: string): number | null => {
  const yuan = parseFloat(raw)
  return Number.isFinite(yuan) && yuan > 0 ? Math.round(yuan * CENTS_PER_YUAN) : null
}
