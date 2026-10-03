import { describe, expect, it } from 'vitest'
import type { UsageBucket, UsageRow } from './api'
import {
  capReached, centsToMc, daysAgo, fmtTime, fmtTok, fmtYuan, groupCost, groupTokens,
  isoDay, parseLimitInput, perAgentSeries, seriesMax, tokenTypeSeries, tokensOf, utcMonthRange,
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
