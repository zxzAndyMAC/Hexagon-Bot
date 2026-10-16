// 票 16c（可访问性抽查，2.4.3 / 2.1.1）：虚拟化时间线里键盘焦点不能「掉到 body」。
// 曾经怎么坏的：Tab 进时间线首行（overscan 行在视口外）→ 浏览器把它滚进视野 → Virtuoso 重算范围、
// 同 key 的行被卸载重挂 → 聚焦元素被移出 DOM，焦点落到 body；再 Tab 又从同一起点进来、再掉，连掉三次才稳。
// 修法：焦点所在元素被移出 DOM（不是点空白、不是切窗口）时，按行 key + 行内第几个可聚焦元素放回去；
// 行已不在渲染范围就退回滚动容器本身（tabindex=0），不留在 body。
const FOCUSABLE = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'

export type FocusAnchor = { rowKey: string; index: number }

/** 记下焦点在哪一行、行内第几个可聚焦元素；不在时间线行里返回 null。 */
export function captureFocusAnchor(target: Element): FocusAnchor | null {
  const row = target.closest<HTMLElement>('[data-timeline-row]')
  const rowKey = row?.dataset.timelineRow
  if (!row || !rowKey) return null
  const index = [...row.querySelectorAll(FOCUSABLE)].indexOf(target)
  return { rowKey, index: Math.max(index, 0) }
}

/** 把焦点放回同一行的同一位置；做不到返回 false。preventScroll：不再触发一次滚动重算。 */
export function restoreFocusAnchor(root: ParentNode, anchor: FocusAnchor): boolean {
  const row = [...root.querySelectorAll<HTMLElement>('[data-timeline-row]')].find((r) => r.dataset.timelineRow === anchor.rowKey)
  if (!row) return false
  const items = [...row.querySelectorAll<HTMLElement>(FOCUSABLE)]
  const el = items[anchor.index] ?? items[0]
  if (!el) return false
  el.focus({ preventScroll: true })
  return document.activeElement === el
}

/** 挂在时间线区域上：被卸载的聚焦元素在下一帧放回原处，退路是滚动容器。返回解绑函数。 */
export function keepTimelineFocus(root: HTMLElement, fallback: () => HTMLElement | null): () => void {
  const onFocusOut = (e: FocusEvent) => {
    if (e.relatedTarget) return
    const target = e.target
    if (!(target instanceof Element)) return
    const anchor = captureFocusAnchor(target)
    if (!anchor) return
    requestAnimationFrame(() => {
      // 只救「元素被移出 DOM」：点空白处 / 切到别的窗口时元素还在，不抢焦点。
      if (target.isConnected) return
      const active = document.activeElement
      if (active && active !== document.body) return
      if (!restoreFocusAnchor(root, anchor)) fallback()?.focus({ preventScroll: true })
    })
  }
  root.addEventListener('focusout', onFocusOut)
  return () => root.removeEventListener('focusout', onFocusOut)
}
