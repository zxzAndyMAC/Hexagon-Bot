import { describe, expect, it } from 'vitest'
import { keyTarget, LEGACY_RAIL_KEY, readSideLayout, resolveDrag, SIDE, SIDE_LAYOUT_KEY, writeSideLayout } from './layout'

function mem(init: Record<string, string> = {}) {
  const m = new Map(Object.entries(init))
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => { m.set(k, v) },
    removeItem: (k: string) => { m.delete(k) },
    has: (k: string) => m.has(k),
    get: (k: string) => m.get(k),
  }
}

describe('侧栏布局持久化（票 02，ADR 0088）', () => {
  it('无记忆时为默认 252 展开', () => {
    expect(readSideLayout(mem())).toEqual({ width: 252, collapsed: false })
  })

  it('旧 hexagon.rail=0 迁成收起，写入新键并删除旧键（只迁一次）', () => {
    const s = mem({ [LEGACY_RAIL_KEY]: '0' })
    expect(readSideLayout(s)).toEqual({ width: SIDE.default, collapsed: true })
    expect(s.has(LEGACY_RAIL_KEY)).toBe(false)
    expect(JSON.parse(s.get(SIDE_LAYOUT_KEY)!)).toEqual({ width: 252, collapsed: true })
    // 第二次读只看新键
    expect(readSideLayout(s)).toEqual({ width: 252, collapsed: true })
  })

  it('旧 hexagon.rail=1 迁成展开', () => {
    const s = mem({ [LEGACY_RAIL_KEY]: '1' })
    expect(readSideLayout(s)).toEqual({ width: 252, collapsed: false })
    expect(s.has(LEGACY_RAIL_KEY)).toBe(false)
  })

  it('新键优先；旧键残留被清掉', () => {
    const s = mem({ [LEGACY_RAIL_KEY]: '0', [SIDE_LAYOUT_KEY]: JSON.stringify({ width: 300, collapsed: false }) })
    expect(readSideLayout(s)).toEqual({ width: 300, collapsed: false })
    expect(s.has(LEGACY_RAIL_KEY)).toBe(false)
  })

  it('越界宽度夹回 200–360，损坏内容回默认', () => {
    expect(readSideLayout(mem({ [SIDE_LAYOUT_KEY]: JSON.stringify({ width: 999, collapsed: true }) }))).toEqual({ width: 360, collapsed: true })
    expect(readSideLayout(mem({ [SIDE_LAYOUT_KEY]: '{oops' }))).toEqual({ width: 252, collapsed: false })
    expect(readSideLayout(mem({ [SIDE_LAYOUT_KEY]: '"x"' }))).toEqual({ width: 252, collapsed: false })
  })

  it('写入取整', () => {
    const s = mem()
    writeSideLayout(s, { width: 280.6, collapsed: false })
    expect(JSON.parse(s.get(SIDE_LAYOUT_KEY)!)).toEqual({ width: 281, collapsed: false })
  })
})

describe('分割线取值（B8 / interaction §1）', () => {
  const side = { min: SIDE.min, max: SIDE.max, snap: SIDE.snap, collapsedWidth: SIDE.collapsed }

  it('范围内跟手，触限停在上下限并标 edge', () => {
    expect(resolveDrag(280.4, side)).toEqual({ width: 280, collapsed: false, edge: 'none' })
    expect(resolveDrag(190, side)).toEqual({ width: 200, collapsed: false, edge: 'min' })
    expect(resolveDrag(400, side)).toEqual({ width: 360, collapsed: false, edge: 'max' })
  })

  it('拖过下限再 40px 吸附成 56 图标列；回拉出吸附区即取消', () => {
    expect(resolveDrag(160, side).edge).toBe('min')
    expect(resolveDrag(159, side)).toEqual({ width: 56, collapsed: true, edge: 'snap' })
    expect(resolveDrag(170, side).collapsed).toBe(false)
  })

  it('不给 snap 的分割线停在下限', () => {
    expect(resolveDrag(10, { min: 160, max: 360 })).toEqual({ width: 160, collapsed: false, edge: 'min' })
  })

  it('键盘：←→ 16、Shift 64、Home/End 到最小/最大，方向随 direction', () => {
    expect(keyTarget('ArrowRight', false, 252, 200, 360)).toBe(268)
    expect(keyTarget('ArrowLeft', true, 252, 200, 360)).toBe(200)
    expect(keyTarget('ArrowRight', true, 340, 200, 360)).toBe(360)
    expect(keyTarget('Home', false, 252, 200, 360)).toBe(200)
    expect(keyTarget('End', false, 252, 200, 360)).toBe(360)
    expect(keyTarget('ArrowLeft', false, 400, 360, 800, -1)).toBe(416)
    expect(keyTarget('Enter', false, 252, 200, 360)).toBeNull()
  })
})
