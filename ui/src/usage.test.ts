import { describe, expect, it } from 'vitest'
import type { UsageBucket, UsageRow } from './api'
import {
  capReached, centsToMc, fmtTok, fmtYuan, groupCost, groupTokens,
  parseLimitInput, perAgentSeries, seriesMax, tokenTypeSeries, tokensOf,
} from './usage'

describe('usage units', () => {
  it('fmtYuan converts millicents to ¥', () => {
    expect(fmtYuan(38200)).toBe('¥0.38')
    expect(fmtYuan(100000)).toBe('¥1.00')
    expect(fmtYuan(null)).toBe('—')
    expect(fmtYuan(undefined)).toBe('—')
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
  const rows: UsageRow[] = [
    { agent_id: 'a0', model: 'm1', stage: null, prompt_tokens: 10, completion_tokens: 1, tool_output_tokens: 0, cost_mc: 100, calls: 2 },
    { agent_id: 'a1', model: 'm1', stage: null, prompt_tokens: 20, completion_tokens: 1, tool_output_tokens: 0, cost_mc: 300, calls: 3 },
    { agent_id: 'a1', model: 'm2', stage: null, prompt_tokens: 0, completion_tokens: 0, tool_output_tokens: 5, cost_mc: 50, calls: 1 },
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
