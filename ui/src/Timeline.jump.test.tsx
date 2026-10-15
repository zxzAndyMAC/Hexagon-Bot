// ui-redesign 票 06：回到底部三态 + 计数播报节流，节点轨点击跳转 / 待决刻度开坞（components C14 / C15）。
import { act, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterAll, afterEach, beforeAll, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { Timeline } from './components/Timeline'
import { useUiStore } from './store'
import type { EventKind, TimelineItem } from './api'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent, scrollerRef }: { data: unknown[]; itemContent: (i: number, row: unknown) => ReactNode; scrollerRef: (node: HTMLElement | null) => void }) =>
    <div data-scroller ref={scrollerRef}>{data.map((row, index) => <div key={index}>{itemContent(index, row)}</div>)}</div>,
}))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const item = (id: number, kind: EventKind, author?: string, payload: Record<string, unknown> = {}): TimelineItem => ({
  event: { id, project_id: 'p', kind, agent_id: author && author !== 'owner' ? author : null, stage_run_id: null, payload, created_at: '2026-10-14T14:02:00Z' },
  message: author ? { id, author, body: `m${id}`, tokens: [], attachments: [], element_refs: [], created_at: '', thinking: '' } : null,
})

const base = { pending: [], streams: {}, thinkings: {}, plans: {}, streamDone: {}, stages: [], projectRoot: null, projectEpoch: 0, timelineFacts: null,
  team: [{ id: 'fe', role: '前端' }, { id: 'qa', role: 'QA' }] } as unknown as Parameters<typeof useUiStore.setState>[0]

beforeAll(async () => { await i18n.changeLanguage('zh-CN') })
afterAll(async () => { await i18n.changeLanguage('en') })
afterEach(() => { vi.useRealTimers(); useUiStore.setState({ timeline: [], streams: {}, pending: [] }) })

async function mount() {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => root.render(<Timeline />))
  return { el, root }
}

/** 往上翻离开真底：用户滚轮 + 位移，松钉且高过窗口 → 按钮出现（showStickButton）。 */
async function scrollAway(el: HTMLElement) {
  const scroller = el.querySelector('[data-scroller]') as HTMLElement
  let top = 800
  Object.defineProperties(scroller, {
    scrollHeight: { configurable: true, value: 1000 }, clientHeight: { configurable: true, value: 200 },
    scrollTop: { configurable: true, get: () => top, set: (v: number) => { top = v } },
  })
  // 先在真底记一次位置，再滚轮往上翻
  await act(async () => { scroller.dispatchEvent(new Event('scroll')) })
  await act(async () => {
    scroller.dispatchEvent(new WheelEvent('wheel', { deltaY: -100, bubbles: true }))
    top = 300
    scroller.dispatchEvent(new Event('scroll'))
  })
}

it('回到底部三态：只有箭头 → N 条新消息 + 头像 → 某某 正在回复；计数不算工具与系统注记', async () => {
  useUiStore.setState({ ...base, timeline: [item(1, 'agent_message', 'fe'), item(2, 'agent_message', 'qa')] })
  const { el, root } = await mount()
  expect(el.querySelector('.jump')).toBeNull()
  await scrollAway(el)
  const jump = () => el.querySelector<HTMLButtonElement>('.jump')!
  expect(jump().dataset.jump).toBe('round')
  expect(jump().getAttribute('aria-label')).toBe(i18n.t('timeline.toBottom'))
  // 对话列正中，最上一块底部表面上方 12px（文档流里没有浮层时即 12）
  expect(jump().style.bottom).toBe('12px')
  await act(async () => useUiStore.setState({ timeline: [...useUiStore.getState().timeline,
    item(3, 'agent_message', 'fe'), item(4, 'tool_called'), item(5, 'system', undefined, { kind: 'note' }), item(6, 'artifact_delivered', 'qa')] }))
  expect(jump().dataset.jump).toBe('fresh')
  expect(jump().getAttribute('aria-label')).toBe(i18n.t('timeline.jumpFresh', { count: 2 }))
  expect(jump().querySelectorAll('[data-slot=avatar]')).toHaveLength(2)
  await act(async () => useUiStore.setState({ streams: { fe: { 0: '正在写' } } }))
  expect(jump().dataset.jump).toBe('replying')
  expect(jump().getAttribute('aria-label')).toBe(i18n.t('timeline.jumpReplying', { name: '前端' }))
  await act(async () => root.unmount())
  el.remove()
})

it('计数变化经 aria-live polite 节流播报（≥2s 一次）', async () => {
  vi.useFakeTimers({ shouldAdvanceTime: false, toFake: ['setTimeout', 'clearTimeout', 'Date'] })
  useUiStore.setState({ ...base, timeline: [item(1, 'agent_message', 'fe')] })
  const { el, root } = await mount()
  await scrollAway(el)
  const live = () => el.querySelector('[data-jump-live]')!
  expect(live().getAttribute('aria-live')).toBe('polite')
  await act(async () => useUiStore.setState({ timeline: [...useUiStore.getState().timeline, item(2, 'agent_message', 'fe')] }))
  await act(async () => { vi.advanceTimersByTime(0) })
  expect(live().textContent).toBe(i18n.t('timeline.newMessages', { count: 1 }))
  await act(async () => useUiStore.setState({ timeline: [...useUiStore.getState().timeline, item(3, 'agent_message', 'fe')] }))
  await act(async () => { vi.advanceTimersByTime(500) })
  // 2s 内不再播报
  expect(live().textContent).toBe(i18n.t('timeline.newMessages', { count: 1 }))
  await act(async () => { vi.advanceTimersByTime(1600) })
  expect(live().textContent).toBe(i18n.t('timeline.newMessages', { count: 2 }))
  await act(async () => root.unmount())
  el.remove()
})

it('节点轨：刻度带层级与可读名，悬停出预览卡，点击跳转并闪烁目标行；待决刻度在最下，点开坞', async () => {
  useUiStore.setState({ ...base, pendingDialogOpen: false, timeline: [
    item(1, 'stage_started', undefined, { stage: '实现' }),
    item(2, 'owner_message', 'owner'),
    { ...item(3, 'artifact_delivered', undefined, { path: 'src/TodoList.tsx' }), event: { ...item(3, 'artifact_delivered').event, agent_id: 'fe', payload: { path: 'src/TodoList.tsx' } } },
    item(4, 'system', undefined, { kind: 'managed_decision', card_kind: 'permission', action: 'allow' }),
  ], pending: [{ id: 'q1', kind: 'permission', agent_id: null, payload: {}, state: 'queued' }] } as never)
  const { el, root } = await mount()
  const ticks = [...el.querySelectorAll<HTMLButtonElement>('[data-tick]')]
  expect(ticks.map(t => [t.dataset.tier, t.dataset.tone ?? ''])).toEqual([['s', ''], ['m', ''], ['e', ''], ['e', 'mg'], ['e', 'warn']])
  expect(ticks[2].getAttribute('aria-label')).toMatch(/^产物交付 src\/TodoList\.tsx，前端，\d\d:\d\d$/)
  await act(async () => { ticks[2].dispatchEvent(new MouseEvent('mouseover', { bubbles: true })) })
  const card = el.querySelector('[data-rail-card]')!
  expect(card.textContent).toContain('src/TodoList.tsx')
  expect(card.textContent).toContain('前端')
  await act(async () => { ticks[2].click() })
  expect(el.querySelector('[data-timeline-event="3"]')!.className).toContain('flash-row')
  await act(async () => { ticks[4].click() })
  expect(useUiStore.getState().pendingDialogOpen).toBe(true)
  await act(async () => root.unmount())
  el.remove()
})
