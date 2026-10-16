// 票 16a：交付卡选中态（canvas#artifact）——该产物正是画布当前页签时挂 data-current / aria-current（1.5px accent 环）。
import { act, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { Timeline } from './components/Timeline'
import { useUiStore } from './store'
import type { TimelineItem } from './api'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent, scrollerRef }: { data: unknown[]; itemContent: (i: number, row: unknown) => ReactNode; scrollerRef: (node: HTMLElement | null) => void }) =>
    <div data-scroller ref={scrollerRef}>{data.map((row, index) => <div key={index}>{itemContent(index, row)}</div>)}</div>,
}))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const delivered = (id: number, path: string): TimelineItem => ({
  event: { id, project_id: 'p', kind: 'artifact_delivered', agent_id: 'fe', stage_run_id: null,
    payload: { path, version: 1, kind: 'code', status: 'pending' }, created_at: '2026-10-14T14:02:00Z' },
  message: null,
})

afterEach(() => { useUiStore.setState({ timeline: [], tabs: [], activeTab: null, canvasOpen: false }) })

it('画布当前页签是该产物时交付卡为选中态，其余卡不是；关画布后取消', async () => {
  useUiStore.setState({ pending: [], streams: {}, thinkings: {}, plans: {}, streamDone: {}, stages: [], projectRoot: null, projectEpoch: 0,
    timelineFacts: null, team: [], artifacts: [], timeline: [delivered(1, 'src/a.ts'), delivered(2, 'src/b.ts')] } as unknown as Parameters<typeof useUiStore.setState>[0])
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => root.render(<Timeline />))
  const cards = () => [...el.querySelectorAll<HTMLButtonElement>('.tl-deliver')]
  expect(cards()).toHaveLength(2)
  expect(cards().some((c) => c.hasAttribute('data-current'))).toBe(false)

  await act(async () => { cards()[1].click() })
  expect(useUiStore.getState().activeTab).toBe('art:src/b.ts')
  expect(cards()[0].hasAttribute('data-current')).toBe(false)
  expect(cards()[1].hasAttribute('data-current')).toBe(true)
  expect(cards()[1].getAttribute('aria-current')).toBe('true')

  await act(async () => useUiStore.setState({ canvasOpen: false }))
  expect(cards()[1].hasAttribute('data-current')).toBe(false)
  await act(async () => root.unmount())
  el.remove()
})
