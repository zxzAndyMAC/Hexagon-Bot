import { describe, expect, it } from 'vitest'
import { cardApp, ordinalCorner, ringPoint, ringPoints, sameImageSize } from './computerTargetRing'

// ui-redesign gap D8（control#action）：目标圈画错位置比不画更坏，所以每一项不可证的事实都必须得到 null。
const base = { op: 'click', x: 800, y: 500, snapshotId: 's1', shotSnapshotId: 's1', imageWidth: 1600, imageHeight: 1000 }

describe('ringPoint', () => {
  it('maps a 16:10 observation 1:1 onto the 16:10 thumbnail', () => {
    expect(ringPoint(base)).toEqual({ left: 0.5, top: 0.5 })
    expect(ringPoint({ ...base, x: 0, y: 0 })).toEqual({ left: 0, top: 0 })
    expect(ringPoint({ ...base, op: 'drag', x: 400, y: 250 })).toEqual({ left: 0.25, top: 0.25 })
  })

  it('applies the object-cover crop: taller images lose top/bottom, wider images lose left/right', () => {
    const square = { ...base, imageWidth: 1000, imageHeight: 1000 }
    expect(ringPoint({ ...square, x: 250, y: 500 })).toEqual({ left: 0.25, top: 0.5 })
    const tall = ringPoint({ ...square, x: 500, y: 750 })!
    expect(tall.top).toBeCloseTo(0.9) // 1000px scaled to 1.6 box heights, 0.3 cropped above
    expect(ringPoint({ ...square, x: 500, y: 100 })).toBeNull() // inside the cropped-away band
    const wide = { ...base, imageWidth: 3200, imageHeight: 1000 }
    expect(ringPoint({ ...wide, x: 1600, y: 500 })).toEqual({ left: 0.5, top: 0.5 })
    expect(ringPoint({ ...wide, x: 100, y: 500 })).toBeNull()
  })

  it('draws nothing when any fact is unproven', () => {
    expect(ringPoint({ ...base, op: 'type' })).toBeNull()
    expect(ringPoint({ ...base, op: 'activate' })).toBeNull()
    expect(ringPoint({ ...base, shotSnapshotId: 's2' })).toBeNull() // thumbnail is a different observation
    expect(ringPoint({ ...base, snapshotId: undefined, shotSnapshotId: undefined })).toBeNull()
    expect(ringPoint({ ...base, imageWidth: undefined })).toBeNull() // pre-D8 evidence row
    expect(ringPoint({ ...base, imageHeight: 0 })).toBeNull()
    expect(ringPoint({ ...base, x: '800' })).toBeNull()
    expect(ringPoint({ ...base, x: Number.NaN })).toBeNull()
    expect(ringPoint({ ...base, x: 1600 })).toBeNull() // native mapPoint rejects x >= width too
    expect(ringPoint({ ...base, y: -1 })).toBeNull()
    expect(ringPoint(base, 0)).toBeNull()
  })
})

// review R4：drag 起点 + 终点；放大模态按图本身宽高比（x/w、y/h）；任一坐标不可证整组不画。
describe('ringPoints', () => {
  const drag = { ...base, op: 'drag', x: 400, y: 250, toX: 1200, toY: 750 }
  const round = (rings: ReturnType<typeof ringPoints>) =>
    rings?.map((r) => ({ ...r, left: Math.round(r.left * 1e6) / 1e6, top: Math.round(r.top * 1e6) / 1e6 })) ?? null
  it('maps both ends of a drag and the single click point', () => {
    expect(ringPoints(base)).toEqual([{ left: 0.5, top: 0.5, role: 'start' }])
    expect(round(ringPoints(drag))).toEqual([{ left: 0.25, top: 0.25, role: 'start' }, { left: 0.75, top: 0.75, role: 'end' }])
  })
  it('uses the image aspect for the uncropped preview', () => {
    const square = { ...drag, imageWidth: 1000, imageHeight: 1000, x: 100, y: 50, toX: 900, toY: 950 }
    expect(ringPoints(square)).toBeNull() // both ends fall in the cropped bands of the 16:10 thumbnail
    expect(round(ringPoints(square, 1))).toEqual([{ left: 0.1, top: 0.05, role: 'start' }, { left: 0.9, top: 0.95, role: 'end' }])
  })
  it('draws nothing when a drag end is unproven', () => {
    expect(ringPoints({ ...drag, toX: undefined })).toBeNull()
    expect(ringPoints({ ...drag, toY: 1000 })).toBeNull()
    expect(ringPoints({ ...drag, toX: Number.NaN })).toBeNull()
    expect(ringPoints({ ...drag, shotSnapshotId: 's2' })).toBeNull()
  })
})

it('sameImageSize only accepts the exact recorded pixel size', () => {
  expect(sameImageSize({ width: 1600, height: 1000 }, 1600, 1000)).toBe(true)
  expect(sameImageSize({ width: 800, height: 500 }, 1600, 1000)).toBe(false)
  expect(sameImageSize({ width: 0, height: 0 }, 0, 0)).toBe(false)
  expect(sameImageSize({ width: 1600, height: 1000 }, undefined, undefined)).toBe(false)
})

it('cardApp prefers the display name and falls back to the bundle id', () => {
  expect(cardApp({ name: '系统设置', title: '隐私与安全性', bundle_id: 'com.apple.systempreferences' })).toEqual({ name: '系统设置', title: '隐私与安全性' })
  expect(cardApp({ name: null, title: 'Window', bundle_id: 'com.example.app' })).toEqual({ name: 'com.example.app', title: 'Window' })
  expect(cardApp({ name: '  ', title: null, bundle_id: 'com.example.app' })).toEqual({ name: 'com.example.app', title: '' })
  expect(cardApp({ name: null, title: 'Window', bundle_id: null })).toBeNull()
  expect(cardApp(null)).toBeNull()
  expect(cardApp('System Settings')).toBeNull()
})

// owner-followups-1015 #30 第 1 项（2026-10-20 负责人裁决）：序号底衬挂在圈外的一角，朝截图中心那一角，
// 这样贴边的圈序号也不会被缩略框 overflow-hidden 裁掉；圈内（目标本身）始终不被遮。
describe('ordinalCorner', () => {
  it('points the ordinal badge toward the image centre', () => {
    expect(ordinalCorner({ left: 0.1, top: 0.1 })).toBe('bottom-right')
    expect(ordinalCorner({ left: 0.9, top: 0.1 })).toBe('bottom-left')
    expect(ordinalCorner({ left: 0.1, top: 0.9 })).toBe('top-right')
    expect(ordinalCorner({ left: 0.9, top: 0.95 })).toBe('top-left')
  })
  it('keeps the default top-right corner at or left of / below the centre', () => {
    expect(ordinalCorner({ left: 0.5, top: 0.5 })).toBe('top-right')
    expect(ordinalCorner({ left: 0, top: 1 })).toBe('top-right')
  })
})
