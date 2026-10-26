// 节点轨鱼眼（owner-followups-1015 票 27，interaction §5「鱼眼」，components C15，foundations §6）。
// 仿 macOS 程序坞放大：指针纵向距离 → 刻度倍数。纯函数，组件只负责量位置、写 --rail-mag。
//
// 取值出处（规范 interaction §5）：
// - max 1.5：阶段刻度 24 → 36，从轨道右内边起向左伸，恰好伸到轨道左缘外 6px = 预览卡留的缝，不压卡。
//   试过 1.8（原生程序坞观感）：阶段刻度 43 宽，伸进预览卡底下被遮，否决。
// - radius 32px：刻度间距 9（C15），半径内约 3.5 道，中心一道最大、两侧各 3 道递减。
// - 升余弦衰减：中心与半径边缘斜率都是 0，指针扫过时倍数连续、没有台阶（线性衰减在中心有尖角，否决）。
import { useEffect, useRef, type PointerEvent as ReactPointerEvent, type RefObject } from 'react'

export const FISHEYE = { max: 1.5, radius: 32 } as const

/** 指针到刻度中心的纵向距离（px）→ 倍数。半径外恒为 1，中心为 max，之间单调递减。 */
export function fisheyeScale(distance: number): number {
  const d = Math.abs(distance)
  if (d >= FISHEYE.radius) return 1
  const falloff = (Math.cos((Math.PI * d) / FISHEYE.radius) + 1) / 2
  return 1 + (FISHEYE.max - 1) * falloff
}

/** 一组刻度中心（与 pointerY 同一坐标系）→ 各自倍数；减少动效下全部为 1（interaction §5）。 */
export function fisheyeScales(centers: readonly number[], pointerY: number, reduced: boolean): number[] {
  return centers.map(center => (reduced ? 1 : fisheyeScale(pointerY - center)))
}

const reducedMotion = () => typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches

/**
 * 把鱼眼挂到节点轨上：pointermove 只记指针，rAF 里每帧算一次，直接写各刻度行内 `--rail-mag`
 * （不走 React state——每帧重渲染整列刻度掉帧）。只对鼠标 / 笔；键盘聚焦不经过这里，所以不放大。
 * 刻度中心在 pointerenter / 刻度数变化时量一次缓存：放大只动伪元素 transform，不改布局，缓存不会过期。
 */
export function useRailFisheye(railRef: RefObject<HTMLElement | null>, count: number) {
  const centers = useRef<number[] | null>(null)
  const pointerY = useRef<number | null>(null)
  const frame = useRef(0)
  const ticks = () => [...(railRef.current?.querySelectorAll<HTMLElement>('[data-tick]') ?? [])]

  const reset = () => {
    if (frame.current) cancelAnimationFrame(frame.current)
    frame.current = 0
    pointerY.current = null
    ticks().forEach(tick => tick.style.removeProperty('--rail-mag'))
  }

  const paint = () => {
    frame.current = 0
    const y = pointerY.current
    if (y == null) return
    const list = ticks()
    if (centers.current?.length !== list.length) {
      centers.current = list.map(tick => { const r = tick.getBoundingClientRect(); return r.top + r.height / 2 })
    }
    fisheyeScales(centers.current, y, false).forEach((scale, i) => list[i].style.setProperty('--rail-mag', scale.toFixed(3)))
  }

  useEffect(() => { centers.current = null }, [count])
  useEffect(() => () => { if (frame.current) cancelAnimationFrame(frame.current) }, [])

  const onPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    // 触摸没有悬停；减少动效下关闭（interaction §5）。
    // 不看 event.buttons：票 27 曾在按住键（选区 / 拖放）时暂停并复位，2026-10-20 负责人裁决
    // 鱼眼没有「拖动时暂停」（票 29），按住键移动与悬停移动一样放大。
    if (event.pointerType === 'touch' || reducedMotion()) {
      if (pointerY.current != null) reset()
      return
    }
    pointerY.current = event.clientY
    if (!frame.current) frame.current = requestAnimationFrame(paint)
  }

  return {
    onPointerEnter: () => { centers.current = null },
    onPointerMove,
    onPointerLeave: reset,
  }
}
