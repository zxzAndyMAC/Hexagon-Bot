import { describe, expect, it } from 'vitest'
import type { UsageRow } from './api'
import { breakdownRows, capReached, centsToMc, fmtYuan, groupCost, parseLimitInput, seriesMax } from './usage'

describe('usage units', () => {
  it('fmtYuan converts millicents to ¥', () => {
    expect(fmtYuan(38200)).toBe('¥0.38')
    expect(fmtYuan(100000)).toBe('¥1.00')
    expect(fmtYuan(null)).toBe('—')
    expect(fmtYuan(undefined)).toBe('—')
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
  const rows: UsageRow[] = [
    { agent_id: 'a0', model: 'm1', prompt_tokens: 1, completion_tokens: 1, tool_output_tokens: 0, cost_mc: 100, calls: 2 },
    { agent_id: 'a1', model: 'm1', prompt_tokens: 1, completion_tokens: 1, tool_output_tokens: 0, cost_mc: 300, calls: 3 },
    { agent_id: 'a1', model: 'm2', prompt_tokens: 0, completion_tokens: 0, tool_output_tokens: 0, cost_mc: 50, calls: 1 },
    { _total: true, spent_mc: 450, limit_cents: 10 },
  ]

  it('breakdownRows drops the _total row', () => {
    expect(breakdownRows(rows)).toHaveLength(3)
  })

  it('groupCost sums by key and sorts desc', () => {
    expect(groupCost(breakdownRows(rows), (r) => r.model ?? '')).toEqual([
      ['m1', 400],
      ['m2', 50],
    ])
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
    expect(seriesMax([{ day: '2025-07-01', cost_mc: 0 }])).toBe(1)
    expect(seriesMax([{ day: '2025-07-01', cost_mc: 7 }, { day: '2025-07-02', cost_mc: 42 }])).toBe(42)
  })
})
