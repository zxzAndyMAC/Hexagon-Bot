import { describe, expect, it } from 'vitest'
import { CANVAS_LAYOUT_KEY, keyTarget, LEGACY_RAIL_KEY, mainView, readCanvasLayout, readSideLayout, resolveDrag, resolveShell, SIDE, SIDE_LAYOUT_KEY, writeCanvasLayout, writeSideLayout } from './layout'

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

// ui-redesign 票 03（ADR 0088 / 0090；interaction §1「区域尺寸」「让位顺序」，components B7）。
describe('画布布局持久化 hexagon.layout.canvas（票 03）', () => {
  it('无记忆时宽度跟随默认 46%、并排', () => {
    expect(readCanvasLayout(mem())).toEqual({ width: null, full: false })
  })
  it('读写宽度与全宽；损坏内容回默认；过窄夹到 360', () => {
    const s = mem()
    writeCanvasLayout(s, { width: 520.4, full: true })
    expect(JSON.parse(s.get(CANVAS_LAYOUT_KEY)!)).toEqual({ width: 520, full: true })
    expect(readCanvasLayout(s)).toEqual({ width: 520, full: true })
    expect(readCanvasLayout(mem({ [CANVAS_LAYOUT_KEY]: '{oops' }))).toEqual({ width: null, full: false })
    expect(readCanvasLayout(mem({ [CANVAS_LAYOUT_KEY]: JSON.stringify({ width: 10, full: false }) }))).toEqual({ width: 360, full: false })
  })
})

describe('拖到上限再推进入全宽（票 03）', () => {
  const lim = { min: 360, max: 700, snap: 40, push: 40 }
  it('上限内停在 max，再推 40 进入 full；拖过下限 40 吸附关闭', () => {
    expect(resolveDrag(720, lim)).toEqual({ width: 700, collapsed: false, edge: 'max' })
    expect(resolveDrag(741, lim)).toEqual({ width: 700, collapsed: false, edge: 'full' })
    expect(resolveDrag(319, lim)).toEqual({ width: 0, collapsed: true, edge: 'snap' })
  })
})

describe('外壳让位 resolveShell（interaction §1）', () => {
  const base = { side: { width: 252, collapsed: false }, sideHidden: false, canvasOpen: false, canvas: { width: null, full: false }, usage: false }
  it('画布关时侧栏按记忆；对话不足 480 时侧栏收成图标列', () => {
    expect(resolveShell({ ...base, windowWidth: 1440 })).toMatchObject({ side: 'expanded', sideWidth: 252, sideForced: false, canvas: 'none' })
    expect(resolveShell({ ...base, windowWidth: 700 })).toMatchObject({ side: 'collapsed', sideWidth: 56, sideForced: true })
  })
  it('默认画布 46% 主区；先压画布到 360', () => {
    const r = resolveShell({ ...base, windowWidth: 1440, canvasOpen: true })
    expect(r).toMatchObject({ canvas: 'side', canvasWidth: Math.round(0.46 * (1440 - 252)), canvasMax: 1440 - 252 - 480 })
    expect(resolveShell({ ...base, windowWidth: 1100, canvasOpen: true })).toMatchObject({ side: 'expanded', canvas: 'side', canvasWidth: 1100 - 252 - 480 })
  })
  it('再收侧栏，最后画布改为 Sheet；对话始终 ≥ 480', () => {
    const r = resolveShell({ ...base, windowWidth: 1000, canvasOpen: true })
    expect(r).toMatchObject({ side: 'collapsed', sideForced: true, canvas: 'side', canvasWidth: 360, canvasMax: 1000 - 56 - 480 })
    expect(resolveShell({ ...base, windowWidth: 800, canvasOpen: true })).toMatchObject({ side: 'collapsed', canvas: 'sheet' })
    for (const w of [600, 900, 1000, 1100, 1300, 1600]) {
      const x = resolveShell({ ...base, windowWidth: w, canvasOpen: true, canvas: { width: 900, full: false } })
      if (x.canvas === 'side') expect(w - x.sideWidth - x.canvasWidth).toBeGreaterThanOrEqual(480)
      else expect(x.canvas).toBe('sheet')
    }
  })
  it('全宽占满主区；用量详情时画布让出；专注模式侧栏隐藏', () => {
    expect(resolveShell({ ...base, windowWidth: 1440, canvasOpen: true, canvas: { width: null, full: true } }).canvas).toBe('full')
    expect(resolveShell({ ...base, windowWidth: 1440, canvasOpen: true, usage: true }).canvas).toBe('none')
    expect(resolveShell({ ...base, windowWidth: 1440, sideHidden: true })).toMatchObject({ side: 'hidden', sideWidth: 0 })
  })
  it('mainView 三态：对话 / 画布全宽 / 用量详情', () => {
    expect(mainView({ canvasOpen: true, canvasLayout: { width: null, full: true }, usageView: false })).toBe('canvas')
    expect(mainView({ canvasOpen: false, canvasLayout: { width: null, full: true }, usageView: false })).toBe('chat')
    expect(mainView({ canvasOpen: true, canvasLayout: { width: null, full: false }, usageView: false })).toBe('chat')
    expect(mainView({ canvasOpen: true, canvasLayout: { width: null, full: true }, usageView: true })).toBe('usage')
  })
})
