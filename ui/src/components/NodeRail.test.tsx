import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import '../i18n'
import { NodeRail } from './NodeRail'
import { FISHEYE } from './railFisheye'
import type { NodeMark } from '../timelineModel'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

// 票 27（owner-followups-1015）：节点轨鱼眼——只对指针，键盘聚焦不放大，离开复位，拖拽中与减少动效下关闭。
const mark = (i: number): NodeMark => ({ eventId: i, rowIdx: i, icon: 'check', label: `n${i}`, tier: 's', type: 'stamp' })
const PITCH = 9 // C15：刻度按钮 9 高

let root: Root | null = null
let host: HTMLElement | null = null

async function mount(count = 5) {
  host = document.createElement('div')
  document.body.appendChild(host)
  root = createRoot(host)
  await act(async () => {
    root!.render(<NodeRail marks={Array.from({ length: count }, (_, i) => mark(i))} highlights={[]} compact={false} onJump={() => {}} />)
  })
  const ticks = [...host.querySelectorAll<HTMLElement>('[data-tick]')]
  // happy-dom 不做布局：钉每道刻度的位置（第 i 道顶边 i*9，高 9）
  ticks.forEach((tick, i) => {
    tick.getBoundingClientRect = () => ({ top: i * PITCH, height: PITCH, bottom: (i + 1) * PITCH, left: 0, right: 24, width: 24, x: 0, y: i * PITCH, toJSON: () => ({}) }) as DOMRect
  })
  return { rail: host.querySelector<HTMLElement>('[data-node-rail]')!, ticks }
}

const mag = (el: HTMLElement) => Number(el.style.getPropertyValue('--rail-mag') || '1')
const frame = () => act(async () => { await new Promise(r => setTimeout(r, 40)) })
const pointer = (el: HTMLElement, type: string, init: PointerEventInit) =>
  el.dispatchEvent(new PointerEvent(type, { bubbles: true, pointerType: 'mouse', ...init }))

afterEach(() => {
  act(() => root?.unmount())
  host?.remove()
  root = null
  host = null
  vi.restoreAllMocks()
})

describe('NodeRail 鱼眼（票 27）', () => {
  it('pointermove：指针下的刻度取最大倍数，邻近递减，远处不变', async () => {
    const { rail, ticks } = await mount()
    pointer(rail, 'pointermove', { clientY: 2 * PITCH + PITCH / 2 })
    await frame()
    expect(mag(ticks[2])).toBeCloseTo(FISHEYE.max)
    expect(mag(ticks[1])).toBeGreaterThan(1)
    expect(mag(ticks[1])).toBeLessThan(FISHEYE.max)
    expect(mag(ticks[1])).toBeCloseTo(mag(ticks[3]))
    expect(mag(ticks[0])).toBeLessThan(mag(ticks[1]))
  })

  it('pointerleave 复位为 1', async () => {
    const { rail, ticks } = await mount()
    pointer(rail, 'pointermove', { clientY: PITCH / 2 })
    await frame()
    expect(mag(ticks[0])).toBeGreaterThan(1)
    pointer(rail, 'pointerout', { clientY: -50, relatedTarget: document.body }) // React 的 onPointerLeave 由 pointerout 合成
    await frame()
    ticks.forEach(tick => expect(mag(tick)).toBe(1))
  })

  it('键盘聚焦与方向键不放大', async () => {
    const { ticks } = await mount()
    await act(async () => { ticks[0].focus() })
    await act(async () => { ticks[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })) })
    await frame()
    expect(document.activeElement).toBe(ticks[1])
    ticks.forEach(tick => expect(mag(tick)).toBe(1))
  })

  it('按键按住（拖拽进行中）暂停鱼眼并复位', async () => {
    const { rail, ticks } = await mount()
    pointer(rail, 'pointermove', { clientY: PITCH / 2 })
    await frame()
    pointer(rail, 'pointermove', { clientY: PITCH / 2, buttons: 1 })
    await frame()
    ticks.forEach(tick => expect(mag(tick)).toBe(1))
  })

  it('触摸指针不放大', async () => {
    const { rail, ticks } = await mount()
    pointer(rail, 'pointermove', { clientY: PITCH / 2, pointerType: 'touch' })
    await frame()
    ticks.forEach(tick => expect(mag(tick)).toBe(1))
  })

  it('prefers-reduced-motion: reduce 下不放大', async () => {
    vi.spyOn(window, 'matchMedia').mockImplementation(query => ({
      matches: query.includes('reduce'), media: query, onchange: null,
      addEventListener: () => {}, removeEventListener: () => {}, addListener: () => {}, removeListener: () => {}, dispatchEvent: () => false,
    }) as MediaQueryList)
    const { rail, ticks } = await mount()
    pointer(rail, 'pointermove', { clientY: PITCH / 2 })
    await frame()
    ticks.forEach(tick => expect(mag(tick)).toBe(1))
  })
})
