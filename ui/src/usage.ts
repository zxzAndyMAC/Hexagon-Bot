import type { UsageBucket, UsageRow } from './api'
import i18n from './i18n'

/** 账本单位：成本以 millicents 计，上限以 cents 计。1 ¥ = 100 cents = 100_000 mc。 */
export const MC_PER_YUAN = 100000
export const MC_PER_CENT = 1000
export const CENTS_PER_YUAN = 100

// ui-audit 票 14（P3-18）：金额固定 CNY 口径（owner 裁决：记账即人民币，
// 多币种可配置化将来单独立项）。Intl currency 让 en/ja 渲染 CN¥，
// 不再与日元 ¥ 混淆；ja 的 JPY 是 ¥、CNY 是 CN¥，歧义即消。
export const fmtYuan = (mc?: number | null, loc?: string) =>
  mc == null
    ? '—'
    : new Intl.NumberFormat(loc ?? i18n.language, { style: 'currency', currency: 'CNY' }).format(mc / MC_PER_YUAN)

// 2026-10-01 原生验收：SQLite datetime('now') 是 UTC，但无时区字符串被浏览器当成本地。
// 只补齐数据库格式；已有 Z/offset 的时间保留原义。时间线的耗时计算也走同一入口。
export const parseTime = (iso: string): number => Date.parse(
  /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?$/.test(iso) ? `${iso.replace(' ', 'T')}Z` : iso,
)

// 票 14（P3-18）：Intl 本地语序——en/ja 不再 MM/DD 美式拼接。
// withSeconds=false → 日期+时分（时间线行）；true → 时分秒（agent 链路步）。
export const fmtTime = (iso: string, withSeconds = false, loc?: string): string => {
  const d = new Date(parseTime(iso))
  if (Number.isNaN(d.getTime())) return ''
  const o: Intl.DateTimeFormatOptions = withSeconds
    ? { hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23' }
    : { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }
  return new Intl.DateTimeFormat(loc ?? i18n.language, o).format(d)
}

// 票 14（P3-18）：usage 区间日界对齐后端 bucket 的 UTC 口径
//（strftime 无 localtime = UTC）。裁决：改前端而非后端加 localtime——
// events/usage bucket 落库即 UTC，后端切 localtime 会把同一物理日
// 拆进不同桶、历史数据失真；代价是 UTC+8 用户「今天」比本地日历
// 早 8h 翻页，属可预期口径差，此处留档。
export const isoDay = (d: Date) => d.toISOString().slice(0, 10)
export const daysAgo = (n: number) => isoDay(new Date(Date.now() - n * 864e5))

/** UTC 月份区间：offset=0 本月首日~今日由调用方拼；offset=-1 上月首~末日。 */
export const utcMonthRange = (offset: 0 | -1): [string, string] => {
  const d = new Date()
  const y = d.getUTCFullYear()
  const m = d.getUTCMonth() + offset
  return [isoDay(new Date(Date.UTC(y, m, 1))), isoDay(new Date(Date.UTC(y, m + 1, 0)))]
}

export const centsToMc = (cents: number) => cents * MC_PER_CENT

export const capReached = (spentMc: number, limitCents: number | null | undefined) =>
  limitCents != null && spentMc >= centsToMc(limitCents)

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

// ───────── ui-redesign 票 12：用量详情（usage#detail / usage#range / usage#agents） ─────────
// ECharts → Recharts 迁移时把整形逻辑从组件里抽出来；日界、分桶口径与票 14 一致（UTC，后端 date()/strftime 无 localtime）。

export const RANGE_PRESETS = ['today', 'yesterday', '7d', '14d', '30d', 'month', 'lastMonth', 'all'] as const
export type RangePreset = (typeof RANGE_PRESETS)[number]
export type UsageRange = { from: string | null; to: string | null; preset: RangePreset | 'custom' }

const DAY_MS = 864e5
const dayOf = (now: number, back: number) => isoDay(new Date(now - back * DAY_MS))

/** 预设 → 含当日的 UTC 闭区间（A23 左侧 2×4）。now 只为测试注入。 */
export function presetRange(preset: RangePreset, now = Date.now()): UsageRange {
  const d = new Date(now)
  const y = d.getUTCFullYear()
  const m = d.getUTCMonth()
  switch (preset) {
    case 'today': return { from: dayOf(now, 0), to: dayOf(now, 0), preset }
    case 'yesterday': return { from: dayOf(now, 1), to: dayOf(now, 1), preset }
    case '7d': return { from: dayOf(now, 6), to: dayOf(now, 0), preset }
    case '14d': return { from: dayOf(now, 13), to: dayOf(now, 0), preset }
    case '30d': return { from: dayOf(now, 29), to: dayOf(now, 0), preset }
    case 'month': return { from: isoDay(new Date(Date.UTC(y, m, 1))), to: dayOf(now, 0), preset }
    case 'lastMonth': return { from: isoDay(new Date(Date.UTC(y, m - 1, 1))), to: isoDay(new Date(Date.UTC(y, m, 0))), preset }
    case 'all': return { from: null, to: null, preset }
  }
}

/** 日历区间：起止颠倒自动纠正（A23 错误行，沿用旧 RangePicker 行为）。 */
export function customRange(a: string, b: string): UsageRange {
  const [from, to] = a <= b ? [a, b] : [b, a]
  return { from, to, preset: 'custom' }
}

const parseDay = (s: string) => Date.parse(`${s}T00:00:00Z`)

/** 紧挨着的等长上一周期（统计卡「较上一周期」）；全部时间没有上一周期。 */
export function previousRange(r: { from: string | null; to: string | null }): { from: string; to: string } | null {
  if (!r.from || !r.to) return null
  const len = Math.round((parseDay(r.to) - parseDay(r.from)) / DAY_MS) + 1
  const from = parseDay(r.from)
  return { from: isoDay(new Date(from - len * DAY_MS)), to: isoDay(new Date(from - DAY_MS)) }
}

/** 区间内每个 UTC 日（含首尾）——图表 X 轴补齐无数据的日子，避免曲线跨空日直连。 */
export function dayAxis(from: string, to: string): string[] {
  const out: string[] = []
  for (let t = parseDay(from); t <= parseDay(to); t += DAY_MS) out.push(isoDay(new Date(t)))
  return out
}

const bucketsOf = (rows: UsageBucket[], axis?: string[]) => axis ?? [...new Set(rows.map((r) => r.bucket))].sort()

export type StackRow = { bucket: string; input: number; output: number; tool: number }

/** Token 趋势三层堆叠（输入 = chart-1，输出 = chart-2，工具输出 = chart-3；foundations §1.4）。 */
export function stackRows(rows: UsageBucket[], axis?: string[]): StackRow[] {
  const out = new Map(bucketsOf(rows, axis).map((k) => [k, { bucket: k, input: 0, output: 0, tool: 0 }]))
  for (const r of rows) {
    const cur = out.get(r.bucket)
    if (!cur) continue
    out.set(r.bucket, {
      ...cur,
      input: cur.input + r.prompt_tokens,
      output: cur.output + r.completion_tokens,
      tool: cur.tool + r.tool_output_tokens,
    })
  }
  return [...out.values()]
}

/** 每 Agent 最近使用（usage#agents）：区间总量前 N 名，每人一列，不堆叠。 */
export function topAgentRows(rows: UsageBucket[], axis: string[] | undefined, n: number): {
  agents: string[]
  rows: ({ bucket: string } & Record<string, number | string>)[]
} {
  const totals = groupTokens(rows, (r) => r.agent_id ?? '—')
  const agents = totals.slice(0, n).map(([id]) => id)
  const keep = new Set(agents)
  const table = new Map(bucketsOf(rows, axis).map((k) => [k, Object.fromEntries(agents.map((a) => [a, 0])) as Record<string, number>]))
  for (const r of rows) {
    const id = r.agent_id ?? '—'
    const cur = table.get(r.bucket)
    if (!cur || !keep.has(id)) continue
    table.set(r.bucket, { ...cur, [id]: cur[id] + tokensOf(r) })
  }
  return { agents, rows: [...table.entries()].map(([bucket, v]) => ({ bucket, ...v })) }
}

export const sumTokens = (rows: UsageBucket[]) => rows.reduce((s, r) => s + tokensOf(r), 0)

/** 「输入 61% · 输出 27%」：整数百分比；无数据回 null。 */
export function tokenShare(rows: UsageBucket[]): { input: number; output: number } | null {
  const total = sumTokens(rows)
  if (total === 0) return null
  const input = rows.reduce((s, r) => s + r.prompt_tokens, 0)
  const output = rows.reduce((s, r) => s + r.completion_tokens, 0)
  return { input: Math.round((input / total) * 100), output: Math.round((output / total) * 100) }
}

/** 环比百分比；上一周期为 0 时回 null（不伪造 ∞%）。 */
export const pctChange = (cur: number, prev: number): number | null =>
  prev > 0 ? ((cur - prev) / prev) * 100 : null

/** 图表尾「较上周」：截至 to 的近 7 个 UTC 日 vs 再往前 7 日；小时桶按日归并。 */
export function weekOverWeek(rows: UsageBucket[], to: string): number | null {
  const end = parseDay(to)
  let cur = 0
  let prev = 0
  for (const r of rows) {
    const back = Math.round((end - parseDay(r.bucket.slice(0, 10))) / DAY_MS)
    if (back >= 0 && back < 7) cur += tokensOf(r)
    else if (back >= 7 && back < 14) prev += tokensOf(r)
  }
  return pctChange(cur, prev)
}

/** X 轴刻度：按天「9/11」（按 UTC 解读桶日期，本地语序），按小时「14:00」。 */
export function bucketTick(bucket: string, granularity: 'day' | 'hour', loc?: string): string {
  if (granularity === 'hour') return bucket.slice(11, 16)
  return new Intl.DateTimeFormat(loc ?? i18n.language, { month: 'numeric', day: 'numeric', timeZone: 'UTC' })
    .format(new Date(parseDay(bucket.slice(0, 10))))
}

/** 时间范围触发钮文字：预设走 usage.p_*，自定义区间「9/11 – 10/10」。 */
export function rangeLabel(range: UsageRange, t: (k: string) => string): string {
  if (range.preset !== 'custom') return t(`usage.p_${range.preset}`)
  return `${bucketTick(range.from ?? '', 'day')} – ${bucketTick(range.to ?? '', 'day')}`
}

/** 按阶段卡头「已完成 3 / 6」：跳过的阶段也算走完。 */
export function stageProgress(stages: { state: string }[]): { done: number; total: number } {
  return { done: stages.filter((s) => s.state === 'done' || s.state === 'skipped').length, total: stages.length }
}

/** 用户输入 ¥ → cents（API 入参）；非法/非正 → null = 清除上限。 */
export const parseLimitInput = (raw: string): number | null => {
  const yuan = parseFloat(raw)
  return Number.isFinite(yuan) && yuan > 0 ? Math.round(yuan * CENTS_PER_YUAN) : null
}
