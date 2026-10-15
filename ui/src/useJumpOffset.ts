// 回到底部的位置与播报（ui-redesign 票 06，components C14，interaction §4，accessibility §3）。
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'

/** 与最上面那块底部表面的间距（C14「底边距输入条上沿 12」）。 */
export const JUMP_GAP_PX = 12
/** 对话列底部依次叠放的表面：触顶横幅、待决坞（展开 / 胶囊）、托管细条（票 08 挂 data-managed-strip）、输入条。 */
const BOTTOM_STACK = '[data-cap-banner], [data-dock], [data-managed-strip], .composer-wrap'

/**
 * 量出按钮的 bottom：时间线区域底边到底部叠放中最上那块上沿的距离 + 12。
 * 现布局里它们都在时间线下方的文档流里（距离 0 → 12）；浮在时间线上的表面（如托管细条）
 * 只要带上 data-managed-strip 就会被让开，不用改这里。
 */
export function useJumpOffset(area: RefObject<HTMLElement | null>): number {
  const [bottom, setBottom] = useState(JUMP_GAP_PX)
  useLayoutEffect(() => {
    const el = area.current
    if (!el) return
    const scope = el.closest('[data-conversation]') ?? document
    const measure = () => {
      const floor = el.getBoundingClientRect().bottom
      const tops = [...scope.querySelectorAll<HTMLElement>(BOTTOM_STACK)]
        .map(node => node.getBoundingClientRect())
        .filter(box => box.height > 0 && box.top < floor)
        .map(box => box.top)
      const next = JUMP_GAP_PX + (tops.length ? floor - Math.min(...tops) : 0)
      setBottom(previous => previous === next ? previous : next)
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    scope.querySelectorAll<HTMLElement>(BOTTOM_STACK).forEach(node => observer.observe(node))
    return () => observer.disconnect()
  })
  return bottom
}

/** 计数播报节流：aria-live polite，≥2s 一次，不抢焦点。 */
export function useThrottledText(text: string, gapMs = 2000): string {
  const [shown, setShown] = useState(text)
  const last = useRef(0)
  useEffect(() => {
    if (text === shown) return
    const wait = Math.max(0, last.current + gapMs - Date.now())
    const timer = setTimeout(() => { last.current = Date.now(); setShown(text) }, wait)
    return () => clearTimeout(timer)
  }, [text, shown, gapMs])
  return shown
}
