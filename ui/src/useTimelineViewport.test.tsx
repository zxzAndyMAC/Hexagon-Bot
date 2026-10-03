import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { useTimelineViewport } from './useTimelineViewport'
import { buildRows } from './timelineModel'
import type { TimelineItem } from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const item = (id: number): TimelineItem => ({ event: { id, kind: 'stage_started', payload: {}, project_id: 'p1', agent_id: null, stage_run_id: null, created_at: '' }, message: null })
type Input = Parameters<typeof useTimelineViewport>[0]
let viewport: ReturnType<typeof useTimelineViewport>
let top = 0, viewportHeight = 200, writes = 0, heightShift = 0
let resizeViewport: () => void
let data: Input
function Harness() {
  viewport = useTimelineViewport(data)
  return <div data-ready={String(viewport.ready)} data-first={viewport.firstItemIndex}
    ref={node => {
      if (!node) { viewport.bindScroller(null); return }
      Object.defineProperties(node, {
        scrollHeight: { configurable: true, get: () => data.rows.length * 100 + heightShift },
        clientHeight: { configurable: true, get: () => viewportHeight },
        scrollTop: { configurable: true, get: () => top, set: (value: number) => { top = value; writes++ } },
      })
      node.getBoundingClientRect = () => new DOMRect(0, 0, 500, viewportHeight)
      viewport.bindScroller(node)
    }}>
    {data.rows.map((row, index) => <div key={row.idx} data-timeline-event={row.idx}
      ref={node => { if (node) node.getBoundingClientRect = () => new DOMRect(0, index * 100 + (index > 0 ? heightShift : 0) - top, 500, 100 + (index === 0 ? heightShift : 0)) }} />)}
  </div>
}
beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => setTimeout(() => callback(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', (id: ReturnType<typeof setTimeout>) => clearTimeout(id))
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: () => void) { resizeViewport = callback }
    observe() {}
    disconnect() {}
  })
  top = 0; viewportHeight = 200; writes = 0; heightShift = 0
  data = { rows: buildRows(Array.from({ length: 10 }, (_, index) => item(index + 1)), 'all'), generation: 1,
    change: 'replace', loading: false, enabled: false, hasBefore: true, hasAfter: false,
    olderLoading: false, olderError: null, loadOlder: vi.fn(async () => {}), loadLatest: vi.fn(async () => {}),
    targetEventId: null, stickReq: 0 }
})
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals() })

it('reveals the initial window only after layout is quiet and the real tail is aligned', async () => {
  data.enabled = true
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    expect(el.firstElementChild?.getAttribute('data-ready')).toBe('false')
    await act(async () => { await vi.advanceTimersByTimeAsync(250) })
    expect(top).toBe(800)
    expect(el.firstElementChild?.getAttribute('data-ready')).toBe('true')
  } finally { await act(async () => root.unmount()) }
})

it('prepend keeps the same source event at the same pixel offset', async () => {
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    top = 150
    await act(async () => viewport.requestOlder())
    expect(data.loadOlder).toHaveBeenCalledOnce()
    data = { ...data, rows: buildRows([item(0), ...Array.from({ length: 10 }, (_, index) => item(index + 1))], 'all'), change: 'prepend' }
    await act(async () => root.render(<Harness />))
    await act(async () => { await vi.advanceTimersByTimeAsync(20) })
    expect(top).toBe(250)
    expect(el.firstElementChild?.getAttribute('data-first')).toBe('999999')
  } finally { await act(async () => root.unmount()) }
})

it('hidden file panes never overwrite the remembered reading position with a zero-size layout', async () => {
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    top = 350; viewportHeight = 0; writes = 0
    await act(async () => viewport.writeTail())
    expect(writes).toBe(0)
    expect(top).toBe(350)
  } finally { await act(async () => root.unmount()) }
})

it('virtualizer scroll corrections never recursively write the tail', async () => {
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    writes = 0; top = 500
    await act(async () => el.firstElementChild?.dispatchEvent(new Event('scroll')))
    expect(writes).toBe(0)
  } finally { await act(async () => root.unmount()) }
})

it('returning from an around window loads latest instead of treating its local tail as current', async () => {
  data.hasAfter = true
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    await act(async () => viewport.holdPin())
    expect(data.loadLatest).toHaveBeenCalledOnce()
  } finally { await act(async () => root.unmount()) }
})

it('continuous live layout growth cannot hide an otherwise loaded window forever', async () => {
  data.enabled = true
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    for (let index = 0; index < 20; index++) {
      await act(async () => { viewport.onHeight(); await vi.advanceTimersByTimeAsync(50) })
    }
    expect(el.firstElementChild?.getAttribute('data-ready')).toBe('true')
    expect(top).toBe(800)
  } finally { await act(async () => root.unmount()) }
})

it('a delayed project identity keeps loading beyond the layout settle bound', async () => {
  data = { ...data, enabled: true, loading: true, rows: [] }
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    await act(async () => { await vi.advanceTimersByTimeAsync(1500) })
    expect(el.firstElementChild?.getAttribute('data-ready')).toBe('false')
  } finally { await act(async () => root.unmount()) }
})

it('late virtualizer height measurements keep the prepend anchor beyond the first frame', async () => {
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    top = 150
    await act(async () => viewport.requestOlder())
    data = { ...data, rows: buildRows([item(0), ...Array.from({ length: 10 }, (_, index) => item(index + 1))], 'all'), change: 'prepend' }
    await act(async () => root.render(<Harness />))
    await act(async () => { await vi.advanceTimersByTimeAsync(100) })
    expect(top).toBe(250)
    await act(async () => { heightShift = 60; viewport.onHeight(); await vi.advanceTimersByTimeAsync(200) })
    expect(top).toBe(310)
    expect(el.querySelector('[data-timeline-event="2"]')?.getBoundingClientRect().top).toBe(-50)
    // A delayed Markdown image may resize after the initial quiet period too.
    await act(async () => { await vi.advanceTimersByTimeAsync(300); heightShift = 80; viewport.onHeight() })
    expect(top).toBe(330)
    expect(el.querySelector('[data-timeline-event="2"]')?.getBoundingClientRect().top).toBe(-50)
  } finally { await act(async () => root.unmount()) }
})

it('lets Virtuoso compensate prepend while the saved anchor is temporarily unmounted', async () => {
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    top = 150
    await act(async () => viewport.requestOlder())
    // 2026-10-02 real Chromium regression: a fallback scrollToIndex here ran
    // before firstItemIndex compensation and doubled the shift to the tail.
    const scrollToIndex = vi.fn()
    viewport.ref.current = { scrollToIndex } as unknown as NonNullable<typeof viewport.ref.current>
    el.querySelector('[data-timeline-event="2"]')?.removeAttribute('data-timeline-event')
    data = { ...data, rows: buildRows([item(0), ...Array.from({ length: 10 }, (_, index) => item(index + 1))], 'all'), change: 'prepend' }
    await act(async () => root.render(<Harness />))
    expect(scrollToIndex).not.toHaveBeenCalled()
    await act(async () => { await vi.advanceTimersByTimeAsync(32) })
    expect(top).toBe(150)
    // The virtualizer finishes its own prepend correction and remounts anchor.
    await act(async () => {
      top = 250
      el.firstElementChild?.children[2].setAttribute('data-timeline-event', '2')
      viewport.onHeight()
      await vi.advanceTimersByTimeAsync(250)
    })
    expect(top).toBe(250)
  } finally { await act(async () => root.unmount()) }
})


it('remembers native prepend compensation when hiding before anchor restoration settles', async () => {
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Harness />))
    top = 150
    await act(async () => el.firstElementChild?.dispatchEvent(new Event('scroll')))
    await act(async () => viewport.requestOlder())
    el.querySelector('[data-timeline-event="2"]')?.removeAttribute('data-timeline-event')
    data = { ...data, rows: buildRows([item(0), ...Array.from({ length: 10 }, (_, index) => item(index + 1))], 'all'), change: 'prepend' }
    await act(async () => root.render(<Harness />))
    // 2026-10-02 actual file-tab regression: compensation scrolls must update
    // the saved position even while they are excluded from user pin intent.
    await act(async () => { top = 250; el.firstElementChild?.dispatchEvent(new Event('scroll')) })
    await act(async () => { viewportHeight = 0; resizeViewport() })
    await act(async () => { top = 0; viewportHeight = 200; resizeViewport() })
    expect(top).toBe(250)
  } finally { await act(async () => root.unmount()) }
})
