import { describe, expect, it } from 'vitest'
import type { UsageBucket, UsageRow } from './api'
import {
  capReached, centsToMc, daysAgo, fmtTime, fmtTok, fmtYuan, groupCost, groupTokens,
  isoDay, parseLimitInput, perAgentSeries, seriesMax, tokenTypeSeries, tokensOf, utcMonthRange,
  RANGE_PRESETS, presetRange, customRange, previousRange, dayAxis, stackRows, topAgentRows,
  sumTokens, tokenShare, pctChange, weekOverWeek, bucketTick, stageProgress,
} from './usage'

describe('usage units', () => {
  it('fmtYuan converts millicents to CNY currency（票 14：Intl 消歧）', () => {
    expect(fmtYuan(38200, 'en')).toBe('CN¥0.38')
    expect(fmtYuan(100000, 'en')).toBe('CN¥1.00')
    // ja 下 CNY 渲染为「元」/CN¥（ICU 版本相关），关键是 ≠ 日元 ¥ 裸符号
    const ja = fmtYuan(100000, 'ja')
    expect(ja).toContain('1.00')
    expect(ja.startsWith('¥')).toBe(false)
    expect(fmtYuan(null)).toBe('—')
    expect(fmtYuan(undefined)).toBe('—')
  })

  it('daysAgo/isoDay 走 UTC 日界（票 14：对齐后端 bucket）', () => {
    // 关键断言：与本地日界无关，永远等于 UTC 日期
    expect(daysAgo(0)).toBe(new Date().toISOString().slice(0, 10))
    expect(daysAgo(1)).toBe(new Date(Date.now() - 864e5).toISOString().slice(0, 10))
    expect(isoDay(new Date(Date.UTC(2026, 0, 5, 23, 59)))).toBe('2026-01-05')
    const [f, l] = utcMonthRange(-1)
    expect(f.endsWith('-01')).toBe(true)
    expect(l >= f).toBe(true)
  })

  it('fmtTime 本地语序（票 14）', () => {
    const d = new Date('2026-01-05T13:04:09Z')
    const en = fmtTime(d.toISOString(), false, 'en')
    // en 语序 = 月/日 + 本地时分
    const hh = String(d.getHours()).padStart(2, '0')
    const mm = String(d.getMinutes()).padStart(2, '0')
    expect(en).toContain(`1/5`)
    expect(en).toContain(`${hh}:${mm}`)
    expect(fmtTime('not-a-date')).toBe('')
    const sec = fmtTime(d.toISOString(), true, 'en')
    expect(sec).toContain(`${hh}:${mm}:${String(d.getSeconds()).padStart(2, '0')}`)
  })

  it('SQLite UTC timestamps display at the same local time as explicit ISO UTC', () => {
    // 2026-10-01 原生验收：06:57 的新消息显示为前一天22:57。
    expect(fmtTime('2026-09-30 22:57:35', false, 'zh-CN'))
      .toBe(fmtTime('2026-09-30T22:57:35Z', false, 'zh-CN'))
  })

  it('fmtTok scales with K/M/B suffixes', () => {
    expect(fmtTok(999)).toBe('999')
    expect(fmtTok(1000)).toBe('1K')
    expect(fmtTok(84400)).toBe('84.4K')
    expect(fmtTok(123456)).toBe('123K')
    expect(fmtTok(2_500_000)).toBe('2.5M')
    expect(fmtTok(3_530_000_000)).toBe('3.53B')
  })

  it('centsToMc: 1 cent = 1000 mc', () => {
    expect(centsToMc(1)).toBe(1000)
    expect(centsToMc(20000)).toBe(20000000) // ¥200 limit
  })

  it('capReached compares spent_mc against limit_cents', () => {
    expect(capReached(999, 1)).toBe(false) // 999 mc < 1 cent (1000 mc)
    expect(capReached(1000, 1)).toBe(true) // at the cap
    expect(capReached(5000, null)).toBe(false)
    expect(capReached(5000, undefined)).toBe(false)
  })
})

describe('usage rows', () => {
  // ADR 0054：_total 哨兵行已拆成 UsageSummary.total——明细行天然纯净，
  // 「滤掉汇总行」这类防御已由类型层取代。
  // Reliability 12: these fixtures have complete, classified model usage.
  const rows: UsageRow[] = [
    { agent_id: 'a0', model: 'm1', stage: null, prompt_tokens: 10, completion_tokens: 1, tool_output_tokens: 0, cost_mc: 100, calls: 2, unknown_requests: 0, legacy_unknown_records: 0, unknown_token_records: 0 },
    { agent_id: 'a1', model: 'm1', stage: null, prompt_tokens: 20, completion_tokens: 1, tool_output_tokens: 0, cost_mc: 300, calls: 3, unknown_requests: 0, legacy_unknown_records: 0, unknown_token_records: 0 },
    { agent_id: 'a1', model: 'm2', stage: null, prompt_tokens: 0, completion_tokens: 0, tool_output_tokens: 5, cost_mc: 50, calls: 1, unknown_requests: 0, legacy_unknown_records: 0, unknown_token_records: 0 },
  ]

  it('groupCost sums cost_mc by key and sorts desc', () => {
    expect(groupCost(rows, (r) => r.model ?? '')).toEqual([
      ['m1', 400],
      ['m2', 50],
    ])
  })

  it('groupTokens sums the three token columns', () => {
    expect(groupTokens(rows, (r) => r.model ?? '')).toEqual([
      ['m1', 32],
      ['m2', 5],
    ])
  })
})

describe('usage series', () => {
  const buckets: UsageBucket[] = [
    { bucket: '2025-07-01', agent_id: 'a0', prompt_tokens: 10, completion_tokens: 5, tool_output_tokens: 0, cost_mc: 1 },
    { bucket: '2025-07-01', agent_id: 'a1', prompt_tokens: 20, completion_tokens: 0, tool_output_tokens: 0, cost_mc: 1 },
    { bucket: '2025-07-02', agent_id: 'a1', prompt_tokens: 0, completion_tokens: 30, tool_output_tokens: 7, cost_mc: 1 },
  ]

  it('tokensOf sums the three token columns', () => {
    expect(tokensOf(buckets[0])).toBe(15)
    expect(tokensOf(buckets[2])).toBe(37)
  })

  it('perAgentSeries fills missing buckets with 0', () => {
    const { labels, series } = perAgentSeries(buckets)
    expect(labels).toEqual(['2025-07-01', '2025-07-02'])
    const a0 = series.find((s) => s.agentId === 'a0')!
    const a1 = series.find((s) => s.agentId === 'a1')!
    expect(a0.points).toEqual([15, 0])
    expect(a1.points).toEqual([20, 37])
  })

  it('tokenTypeSeries aggregates across agents', () => {
    const { labels, series } = tokenTypeSeries(buckets)
    expect(labels).toEqual(['2025-07-01', '2025-07-02'])
    expect(series[0]).toEqual([30, 0]) // prompt
    expect(series[1]).toEqual([5, 30]) // completion
    expect(series[2]).toEqual([0, 7]) // tool_output
  })
})

describe('usage input', () => {
  it('parseLimitInput: ¥ → cents, empty/invalid clears', () => {
    expect(parseLimitInput('200')).toBe(20000)
    expect(parseLimitInput('0.5')).toBe(50)
    expect(parseLimitInput('')).toBeNull()
    expect(parseLimitInput('abc')).toBeNull()
    expect(parseLimitInput('-3')).toBeNull()
    expect(parseLimitInput('0')).toBeNull()
  })

  it('seriesMax never returns 0', () => {
    expect(seriesMax([])).toBe(1)
    expect(seriesMax([{ bucket: '2025-07-01', agent_id: 'a', prompt_tokens: 0, completion_tokens: 0, tool_output_tokens: 0, cost_mc: 0 }])).toBe(1)
    expect(seriesMax([
      { bucket: '2025-07-01', agent_id: 'a', prompt_tokens: 0, completion_tokens: 0, tool_output_tokens: 0, cost_mc: 7 },
      { bucket: '2025-07-02', agent_id: 'a', prompt_tokens: 0, completion_tokens: 0, tool_output_tokens: 0, cost_mc: 42 },
    ])).toBe(42)
  })
})

// 票 15（P3）：AgentTab 步骤摘要的对象入参不再渲染 [object Object]
describe('toolInputSummary（票 15）', () => {
  it('path 优先；对象 input 取 path 或序列化截断；标量直用', async () => {
    const { toolInputSummary } = await import('./agentSteps')
    expect(toolInputSummary({ path: 'src/a.ts', input: { path: 'x' } })).toBe('src/a.ts')
    expect(toolInputSummary({ input: { path: 'README.md', content: 'x' } })).toBe('README.md')
    // exec-cards 票 02 行为变更：bash 的 cmd 直接取命令文本——
    // 序列化会把命令裹成 {"cmd":"…"} 噪声，卡头/chip 都读不出命令本体
    expect(toolInputSummary({ input: { cmd: 'ls' } })).toBe('ls')
    expect(toolInputSummary({ input: 'plain text' })).toBe('plain text')
    expect(toolInputSummary({})).toBe('')
    const long = toolInputSummary({ input: { blob: 'x'.repeat(200) } })
    expect(long.endsWith('…')).toBe(true)
    expect(long.length).toBeLessThanOrEqual(120)
  })
})

// ui-redesign 票 12（usage#detail / usage#range / usage#agents）：图表换 Recharts 后，
// 时间范围、分桶补齐、环比、每 Agent 前 N 名都收进 usage.ts 纯函数；日界仍是 UTC（票 14 裁决不变）。
describe('usage range（票 12，A23）', () => {
  // 2026-10-10 15:00 UTC——UTC+8 本地已是 10/11 凌晨，断言仍按 UTC 日界
  const now = Date.UTC(2026, 9, 10, 15, 0)

  it('presetRange 八个预设都按 UTC 日界给出含当日的闭区间', () => {
    expect(RANGE_PRESETS).toEqual(['today', 'yesterday', '7d', '14d', '30d', 'month', 'lastMonth', 'all'])
    expect(presetRange('today', now)).toEqual({ from: '2026-10-10', to: '2026-10-10', preset: 'today' })
    expect(presetRange('yesterday', now)).toEqual({ from: '2026-10-09', to: '2026-10-09', preset: 'yesterday' })
    expect(presetRange('7d', now)).toEqual({ from: '2026-10-04', to: '2026-10-10', preset: '7d' })
    expect(presetRange('14d', now)).toEqual({ from: '2026-09-27', to: '2026-10-10', preset: '14d' })
    expect(presetRange('30d', now)).toEqual({ from: '2026-09-11', to: '2026-10-10', preset: '30d' })
    expect(presetRange('month', now)).toEqual({ from: '2026-10-01', to: '2026-10-10', preset: 'month' })
    expect(presetRange('lastMonth', now)).toEqual({ from: '2026-09-01', to: '2026-09-30', preset: 'lastMonth' })
    expect(presetRange('all', now)).toEqual({ from: null, to: null, preset: 'all' })
  })

  it('customRange 起止颠倒自动纠正（A23 错误行：沿用现有行为，不报错）', () => {
    expect(customRange('2026-10-05', '2026-09-30')).toEqual({ from: '2026-09-30', to: '2026-10-05', preset: 'custom' })
    expect(customRange('2026-09-30', '2026-09-30')).toEqual({ from: '2026-09-30', to: '2026-09-30', preset: 'custom' })
  })

  it('previousRange 取紧挨着的等长区间；全部时间没有上一周期', () => {
    expect(previousRange({ from: '2026-09-11', to: '2026-10-10' })).toEqual({ from: '2026-08-12', to: '2026-09-10' })
    expect(previousRange({ from: '2026-10-10', to: '2026-10-10' })).toEqual({ from: '2026-10-09', to: '2026-10-09' })
    expect(previousRange({ from: null, to: null })).toBeNull()
  })

  it('dayAxis 补齐区间内每个 UTC 日（无数据日也占位）', () => {
    expect(dayAxis('2026-09-29', '2026-10-02')).toEqual(['2026-09-29', '2026-09-30', '2026-10-01', '2026-10-02'])
    expect(dayAxis('2026-10-02', '2026-10-02')).toEqual(['2026-10-02'])
  })
})

describe('usage chart rows（票 12，E13）', () => {
  const b = (bucket: string, agent: string, p: number, c = 0, t = 0): UsageBucket =>
    ({ bucket, agent_id: agent, prompt_tokens: p, completion_tokens: c, tool_output_tokens: t, cost_mc: 0 })
  const buckets = [b('2026-10-01', 'a0', 10, 5, 1), b('2026-10-01', 'a1', 20), b('2026-10-03', 'a1', 0, 30, 7)]

  it('stackRows：按轴补零，输入 / 输出 / 工具输出三层', () => {
    expect(stackRows(buckets, ['2026-10-01', '2026-10-02', '2026-10-03'])).toEqual([
      { bucket: '2026-10-01', input: 30, output: 5, tool: 1 },
      { bucket: '2026-10-02', input: 0, output: 0, tool: 0 },
      { bucket: '2026-10-03', input: 0, output: 30, tool: 7 },
    ])
  })

  it('stackRows 不给轴时取数据里出现的桶（按小时粒度）', () => {
    const rows = stackRows([b('2026-10-01 09:00', 'a0', 1), b('2026-10-01 08:00', 'a0', 2)])
    expect(rows.map((r) => r.bucket)).toEqual(['2026-10-01 08:00', '2026-10-01 09:00'])
  })

  it('topAgentRows：只取总量前 N 名，按总量降序，各自一列', () => {
    const { agents, rows } = topAgentRows(buckets, ['2026-10-01', '2026-10-02', '2026-10-03'], 1)
    expect(agents).toEqual(['a1'])
    expect(rows).toEqual([
      { bucket: '2026-10-01', a1: 20 },
      { bucket: '2026-10-02', a1: 0 },
      { bucket: '2026-10-03', a1: 37 },
    ])
  })

  it('sumTokens / tokenShare：区间合计与输入输出占比（整数百分比）', () => {
    expect(sumTokens(buckets)).toBe(73)
    expect(tokenShare(buckets)).toEqual({ input: 41, output: 48 })
    expect(tokenShare([])).toBeNull()
  })

  it('pctChange：上一周期为 0 时不给环比（不伪造 ∞%）', () => {
    expect(pctChange(118.4, 100)).toBeCloseTo(18.4)
    expect(pctChange(50, 100)).toBe(-50)
    expect(pctChange(10, 0)).toBeNull()
  })

  it('weekOverWeek：截至 to 的近 7 个 UTC 日 vs 再往前 7 日；小时桶按日归并', () => {
    const rows = [
      b('2026-09-27', 'a', 100), // 前一周（9/27–10/03）
      b('2026-10-04 10:00', 'a', 60), b('2026-10-10', 'a', 60), // 本周（10/04–10/10）
      b('2026-09-26', 'a', 999), // 两周之外，不计
    ]
    expect(weekOverWeek(rows, '2026-10-10')).toBeCloseTo(20)
    expect(weekOverWeek([b('2026-10-10', 'a', 5)], '2026-10-10')).toBeNull()
  })

  it('bucketTick：按天给 月/日（UTC），按小时给 时:分', () => {
    expect(bucketTick('2026-09-11', 'day', 'en')).toBe('9/11')
    expect(bucketTick('2026-09-11 14:00', 'hour', 'en')).toBe('14:00')
  })

  it('stageProgress：已完成 / 总数（跳过算完成）', () => {
    const s = (stage: string, state: 'done' | 'active' | 'pending' | 'skipped') => ({ run_id: 'r', stage, seq: 0, state })
    expect(stageProgress([s('spec', 'done'), s('ia', 'skipped'), s('ui', 'active'), s('impl', 'pending')]))
      .toEqual({ done: 2, total: 4 })
  })
})
