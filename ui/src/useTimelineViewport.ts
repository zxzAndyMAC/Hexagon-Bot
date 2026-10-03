import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { VirtuosoHandle } from 'react-virtuoso'
import { prependedRowCount, rowContainsEvent, rowAnchorEvent, rowKey, type Row } from './timelineModel'
import { listOverflows, pinAfterScroll, tailScrollTop } from './timelineStick'

type Input = {
  rows: Row[]; generation: number; change: string; loading: boolean; enabled: boolean
  hasBefore: boolean; hasAfter: boolean; olderLoading: boolean; olderError: string | null
  loadOlder: () => Promise<void>; loadLatest: () => Promise<void>
  targetEventId: number | null; stickReq: number
}
type Anchor = { eventId: number; rowKey?: string; leaf: boolean; offset: number; top: number }
const FIRST_INDEX = 1_000_000
const PRELOAD_PX = 480

/** Owner Q1–Q6, 2026-10-02: paging and layout are separate readiness boundaries.
 * Event anchors survive folded-row regrouping; raw event counts cannot anchor a
 * virtual list. Hidden file panes never write a zero-sized viewport's scrollTop.
 */
export function useTimelineViewport(input: Input) {
  const current = useRef(input)
  current.current = input
  const ref = useRef<VirtuosoHandle>(null)
  const scroller = useRef<HTMLElement | null>(null)
  const detach = useRef<(() => void) | null>(null)
  const pinnedRef = useRef(true)
  const ownScroll = useRef(false)
  const userUntil = useRef(0)
  const pointerDown = useRef(false)
  const previousTop = useRef(0)
  const anchor = useRef<Anchor | null>(null)
  const restoring = useRef(false)
  const pendingAnchor = useRef(false)
  const restoreFrame = useRef(0)
  const hiddenTop = useRef<number | null>(null)
  const quietSince = useRef(0)
  const [pinned, setPinned] = useState(true)
  const [overflow, setOverflow] = useState(false)
  const [readyGeneration, setReadyGeneration] = useState(input.enabled ? -1 : input.generation)
  const [layout, setLayout] = useState({ rows: input.rows, generation: input.generation, firstIndex: FIRST_INDEX })
  if (layout.rows !== input.rows || layout.generation !== input.generation) {
    const firstIndex = layout.generation !== input.generation ? FIRST_INDEX
      : input.change === 'prepend' ? Math.max(1, layout.firstIndex - prependedRowCount(layout.rows, input.rows)) : layout.firstIndex
    setLayout({ rows: input.rows, generation: input.generation, firstIndex })
  }
  const visible = () => !!scroller.current && scroller.current.clientHeight > 0
  const capture = useCallback(() => {
    const el = scroller.current
    if (!el || !visible()) return
    const top = el.getBoundingClientRect().top
    let candidate: HTMLElement | undefined
    for (const node of el.querySelectorAll<HTMLElement>('[data-timeline-row], [data-timeline-event]')) {
      const box = node.getBoundingClientRect()
      if (box.height <= 0 || box.bottom <= top || box.top >= top + el.clientHeight) continue
      if (!candidate || box.top <= top) candidate = node
    }
    if (candidate) {
      const key = candidate.closest<HTMLElement>('[data-timeline-row]')?.dataset.timelineRow
      const row = current.current.rows.find(row => rowKey(row) === key)
      const leaf = candidate.dataset.timelineEvent != null
      const eventId = leaf ? Number(candidate.dataset.timelineEvent) : row ? rowAnchorEvent(row) : undefined
      if (eventId != null) anchor.current = { eventId, rowKey: key, leaf, offset: candidate.getBoundingClientRect().top - top, top: el.scrollTop }
    }
  }, [])
  const writeTop = useCallback((top: number) => {
    const el = scroller.current
    if (!el || !visible() || Math.abs(el.scrollTop - top) <= 1) return
    ownScroll.current = true
    el.scrollTop = top
    ownScroll.current = false
    previousTop.current = el.scrollTop
  }, [])
  const writeTail = useCallback(() => {
    const el = scroller.current
    if (el && pinnedRef.current && visible()) writeTop(tailScrollTop(el.scrollHeight, el.clientHeight))
  }, [writeTop])
  const restoreAnchor = useCallback(() => {
    const el = scroller.current, saved = anchor.current
    if (!el || !saved || !visible() || pinnedRef.current) return
    const row = current.current.rows.find(row => rowContainsEvent(row, saved.eventId))
    const node = (saved.leaf ? el.querySelector<HTMLElement>(`[data-timeline-event="${saved.eventId}"]`) : null)
      ?? (saved.rowKey ? el.querySelector<HTMLElement>(`[data-timeline-row="${saved.rowKey}"]`) : null)
      ?? (row ? el.querySelector<HTMLElement>(`[data-timeline-row="${rowKey(row)}"]`) : null)
    if (node) {
      writeTop(el.scrollTop + node.getBoundingClientRect().top - el.getBoundingClientRect().top - saved.offset)
      if (!saved.leaf && node.dataset.timelineRow) anchor.current = { ...saved, rowKey: node.dataset.timelineRow }
    }
  }, [writeTop])
  const settleAnchor = useCallback(() => {
    cancelAnimationFrame(restoreFrame.current)
    let lastHeight = scroller.current?.scrollHeight ?? 0
    quietSince.current = performance.now()
    const step = () => {
      if (!restoring.current) return
      const el = scroller.current
      if (el && visible()) {
        if (lastHeight !== el.scrollHeight) { lastHeight = el.scrollHeight; quietSince.current = performance.now() }
        restoreAnchor()
        const id = anchor.current?.eventId
        if (id != null && (el.querySelector(`[data-timeline-event="${id}"]`) || el.querySelector(`[data-timeline-row="${anchor.current?.rowKey}"]`)) && performance.now() - quietSince.current >= 180) {
          restoring.current = false
          capture()
          return
        }
      }
      restoreFrame.current = requestAnimationFrame(step)
    }
    restoreFrame.current = requestAnimationFrame(step)
  }, [capture, restoreAnchor])
  const releasePin = useCallback(() => {
    pinnedRef.current = false
    setPinned(false)
    capture()
  }, [capture])
  const holdPin = useCallback(() => {
    if (current.current.hasAfter) { void current.current.loadLatest(); return }
    pinnedRef.current = true
    setPinned(true)
    anchor.current = null
    ref.current?.scrollToIndex({ index: 'LAST', align: 'end' })
    writeTail()
    requestAnimationFrame(writeTail)
  }, [writeTail])
  const requestOlder = useCallback(() => {
    const state = current.current
    if (!state.hasBefore || state.loading || state.olderLoading || state.olderError) return
    releasePin()
    capture()
    pendingAnchor.current = true
    void state.loadOlder()
  }, [capture, releasePin])
  const centerEvent = useCallback((id: number) => {
    const el = scroller.current
    if (!el || !visible()) return
    const node = el.querySelector<HTMLElement>(`[data-timeline-event="${id}"], [data-timeline-result="${id}"]`)
    if (!node) return
    const box = node.getBoundingClientRect()
    writeTop(el.scrollTop + box.top - el.getBoundingClientRect().top - Math.max(0, (el.clientHeight - box.height) / 2))
  }, [writeTop])
  const jumpToRow = useCallback((index: number, targetId?: number) => {
    releasePin()
    anchor.current = null
    ref.current?.scrollToIndex({ index, align: 'center', behavior: 'auto' })
    const generation = current.current.generation
    requestAnimationFrame(() => {
      if (generation !== current.current.generation) return
      if (targetId != null) centerEvent(targetId)
      requestAnimationFrame(() => { if (generation !== current.current.generation) return; if (targetId != null) centerEvent(targetId); capture() })
    })
  }, [capture, centerEvent, releasePin])
  const onHeight = useCallback(() => {
    quietSince.current = performance.now()
    if (pinnedRef.current) writeTail()
    else if (restoring.current || (!pointerDown.current && performance.now() >= userUntil.current)) restoreAnchor()
  }, [restoreAnchor, writeTail])
  const bindScroller = useCallback((node: HTMLElement | Window | null) => {
    const el = node instanceof HTMLElement ? node : null
    if (scroller.current === el) return
    detach.current?.()
    scroller.current = el
    if (!el) return
    el.style.overflowAnchor = 'none'
    const intent = () => { userUntil.current = performance.now() + 250 }
    const wheel = (event: WheelEvent) => { restoring.current = false; intent(); if (event.deltaY < 0 && el.scrollTop <= PRELOAD_PX) requestOlder() }
    const pointer = () => { restoring.current = false; pointerDown.current = true; intent() }
    const release = () => { pointerDown.current = false; requestAnimationFrame(capture) }
    const key = (event: KeyboardEvent) => {
      if (['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' '].includes(event.key)) { restoring.current = false; intent() }
      if (['ArrowUp', 'PageUp', 'Home'].includes(event.key) && el.scrollTop <= PRELOAD_PX) requestOlder()
    }
    const scroll = () => {
      if (!visible()) return
      const oldTop = previousTop.current
      previousTop.current = el.scrollTop
      // 2026-10-02 issue15 native file-tab regression: Virtuoso's prepend
      // scroll is not user intent, but it is the position to save on hide.
      // Returning before this assignment restored the pre-prepend offset.
      if (restoring.current) return
      setOverflow(listOverflows(el.scrollHeight, el.clientHeight))
      const userMoved = pointerDown.current || performance.now() < userUntil.current
      if (userMoved) intent()
      const next = pinAfterScroll({ pinned: pinnedRef.current, own: ownScroll.current, userMoved,
        scrollTop: el.scrollTop, previousScrollTop: oldTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight })
      // An around window's end is not the project's latest event.
      pinnedRef.current = next && !current.current.hasAfter
      setPinned(pinnedRef.current)
      if (userMoved && !ownScroll.current) {
        capture()
        if (el.scrollTop < oldTop && el.scrollTop <= PRELOAD_PX) requestOlder()
      }
      // Native 2026-10-01: never writeTail from scroll. WebKit's asynchronous
      // virtualizer corrections otherwise recursively produce more scroll events.
    }
    el.addEventListener('scroll', scroll, { passive: true })
    el.addEventListener('wheel', wheel, { passive: true })
    el.addEventListener('pointerdown', pointer)
    window.addEventListener('pointerup', release)
    window.addEventListener('pointercancel', release)
    el.addEventListener('keydown', key)
    const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => {
      if (!visible()) { hiddenTop.current ??= previousTop.current; return }
      if (hiddenTop.current != null) {
        if (!pinnedRef.current) { writeTop(hiddenTop.current); requestAnimationFrame(restoreAnchor) }
        hiddenTop.current = null
      }
      setOverflow(listOverflows(el.scrollHeight, el.clientHeight))
      writeTail()
    })
    resize?.observe(el)
    detach.current = () => {
      el.removeEventListener('scroll', scroll); el.removeEventListener('wheel', wheel)
      el.removeEventListener('pointerdown', pointer); el.removeEventListener('keydown', key)
      window.removeEventListener('pointerup', release); window.removeEventListener('pointercancel', release)
      resize?.disconnect()
    }
  }, [capture, requestOlder, restoreAnchor, writeTail, writeTop])
  useEffect(() => () => { detach.current?.(); cancelAnimationFrame(restoreFrame.current) }, [])
  const seenGeneration = useRef(input.generation)
  useLayoutEffect(() => {
    if (seenGeneration.current !== input.generation) {
      seenGeneration.current = input.generation
      pinnedRef.current = input.targetEventId == null
      setPinned(pinnedRef.current)
      anchor.current = null
      restoring.current = false
      pendingAnchor.current = false
      hiddenTop.current = null
    }
    if (input.change === 'prepend' && pendingAnchor.current) {
      pendingAnchor.current = false
      restoring.current = true
      // 2026-10-02 issue15 Chromium regression: the anchor is briefly unmounted
      // while Virtuoso applies firstItemIndex compensation. scrollToIndex here
      // doubled that shift and jumped to the tail. Let Virtuoso remount it;
      // settleAnchor only corrects the observed pixel offset, without a timer
      // that guesses when the virtualizer has finished its own compensation.
      restoreAnchor()
      settleAnchor()
      return
    }
    writeTail()
  }, [input.rows, input.generation, input.targetEventId, input.change, capture, restoreAnchor, settleAnchor, writeTail])
  useEffect(() => {
    if (!input.enabled || input.loading || readyGeneration === input.generation) return
    let frame = 0, stopped = false, positioned = false, height = -1
    const started = performance.now()
    quietSince.current = performance.now()
    const settle = () => {
      if (stopped) return
      const el = scroller.current
      if (el && visible() && document.visibilityState !== 'hidden') {
        if (!positioned) {
          const index = input.targetEventId == null ? -1 : input.rows.findIndex(row => rowContainsEvent(row, input.targetEventId!))
          if (index >= 0) { pinnedRef.current = false; setPinned(false); ref.current?.scrollToIndex({ index, align: 'center' }) }
          else if (input.rows.length) ref.current?.scrollToIndex({ index: 'LAST', align: 'end' })
          positioned = true
        }
        if (input.targetEventId != null) centerEvent(input.targetEventId)
        writeTail()
        if (height !== el.scrollHeight) { height = el.scrollHeight; quietSince.current = performance.now() }
        const aligned = !pinnedRef.current || Math.abs(el.scrollTop - tailScrollTop(el.scrollHeight, el.clientHeight)) <= 2
        // A live footer can grow forever. Bound the initial settling period;
        // always align the current real tail before revealing the static window.
        if (aligned && (performance.now() - quietSince.current >= 120 || performance.now() - started >= 800)) {
          setReadyGeneration(input.generation)
          setOverflow(listOverflows(el.scrollHeight, el.clientHeight))
          capture()
          return
        }
      }
      frame = requestAnimationFrame(settle)
    }
    frame = requestAnimationFrame(settle)
    return () => { stopped = true; cancelAnimationFrame(frame) }
  }, [input.enabled, input.loading, input.generation, input.targetEventId, input.rows, readyGeneration, capture, centerEvent, writeTail])
  const seenStick = useRef(input.stickReq)
  useEffect(() => {
    if (seenStick.current === input.stickReq) return
    seenStick.current = input.stickReq
    if (input.stickReq > 0) holdPin()
  }, [input.stickReq, holdPin])
  return { ref, bindScroller, firstItemIndex: layout.firstIndex, pinned, overflow,
    ready: !input.enabled || (!input.loading && readyGeneration === input.generation),
    holdPin, releasePin, jumpToRow, onHeight, requestOlder, writeTail }
}
