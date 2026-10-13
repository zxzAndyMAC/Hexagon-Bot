import { describe, expect, it } from 'vitest'
import type { UsageTotal } from './api'
import { usageTier } from './usageEntry'

const total = (spent_mc: number, limit_cents: number | null): UsageTotal => ({
  reserved_mc: 0, spent_mc, limit_cents, tokens: 0, unknown_requests: 0, legacy_unknown_records: 0, unknown_token_records: 0,
})

describe('用量入口三档（B4，票 02）', () => {
  it('未读到时只显示图标', () => {
    expect(usageTier(null)).toEqual({ tier: 'unknown', pct: null })
  })
  it('未设上限为 ok 且无百分比', () => {
    expect(usageTier(total(5_000_000, null))).toEqual({ tier: 'ok', pct: null })
  })
  it('<80% ok，≥80% warn（向下取整），触顶 cap', () => {
    // 上限 ¥100 = 10000 cents = 10_000_000 mc
    expect(usageTier(total(3_860_000, 10000))).toEqual({ tier: 'ok', pct: 38 })
    expect(usageTier(total(7_996_000, 10000))).toEqual({ tier: 'ok', pct: 79 })
    expect(usageTier(total(8_600_000, 10000))).toEqual({ tier: 'warn', pct: 86 })
    expect(usageTier(total(10_000_000, 10000))).toEqual({ tier: 'cap', pct: 100 })
    expect(usageTier(total(12_000_000, 10000)).tier).toBe('cap')
    expect(usageTier(total(0, 0)).tier).toBe('cap')
  })
})
