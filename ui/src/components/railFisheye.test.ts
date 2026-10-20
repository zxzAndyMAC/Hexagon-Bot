import { describe, expect, it } from 'vitest'
import { FISHEYE, fisheyeScale, fisheyeScales } from './railFisheye'

describe('节点轨鱼眼 距离→倍数（owner-followups-1015 票 27，interaction §5）', () => {
  it('指针正对刻度中心时取最大倍数', () => {
    expect(fisheyeScale(0)).toBe(FISHEYE.max)
  })

  it('影响半径上与半径外为 1（不放大）', () => {
    expect(fisheyeScale(FISHEYE.radius)).toBe(1)
    expect(fisheyeScale(FISHEYE.radius + 1)).toBe(1)
    expect(fisheyeScale(1000)).toBe(1)
  })

  it('距离取绝对值：上下对称', () => {
    expect(fisheyeScale(-10)).toBe(fisheyeScale(10))
  })

  it('随距离单调不增，且半径内严格大于 1', () => {
    let prev = Infinity
    for (let d = 0; d <= FISHEYE.radius + 8; d += 0.5) {
      const s = fisheyeScale(d)
      expect(s).toBeLessThanOrEqual(prev)
      expect(s).toBeGreaterThanOrEqual(1)
      expect(s).toBeLessThanOrEqual(FISHEYE.max)
      if (d < FISHEYE.radius) expect(s).toBeGreaterThan(1)
      prev = s
    }
  })

  it('一组刻度：最近的一道最大，向两侧递减', () => {
    const centers = [0, 9, 18, 27, 36]
    const scales = fisheyeScales(centers, 18, false)
    expect(scales[2]).toBe(FISHEYE.max)
    expect(scales[1]).toBe(scales[3])
    expect(scales[1]).toBeLessThan(scales[2])
    expect(scales[0]).toBeLessThan(scales[1])
  })

  it('减少动效下全部为 1', () => {
    expect(fisheyeScales([0, 9, 18], 9, true)).toEqual([1, 1, 1])
  })

  it('不改入参数组', () => {
    const centers = Object.freeze([0, 9]) as readonly number[]
    expect(() => fisheyeScales(centers, 0, false)).not.toThrow()
  })
})
