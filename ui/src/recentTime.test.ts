import { describe, expect, it } from 'vitest'
import { recentTime } from './recentTime'

describe('recentTime（启动页最近项目，票 10 E11 补设计）', () => {
  const now = Date.UTC(2026, 9, 13, 12, 0, 0)
  const secs = (ms: number) => Math.floor(ms / 1000)
  it('缺时间不显示', () => {
    expect(recentTime(0, now, 'en')).toBe('')
  })
  it('7 天内用相对时间', () => {
    expect(recentTime(secs(now - 2 * 3600_000), now, 'en')).toBe('2 hours ago')
    expect(recentTime(secs(now - 24 * 3600_000), now, 'en')).toBe('yesterday')
    expect(recentTime(secs(now - 2 * 3600_000), now, 'zh-CN')).toBe('2小时前')
  })
  it('更早用月 / 日', () => {
    expect(recentTime(secs(Date.UTC(2026, 8, 28, 12)), now, 'en')).toBe('9/28')
  })
})
