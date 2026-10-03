import { act, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { Timeline } from './components/Timeline'
import { useUiStore } from './store'
import type { TimelineItem } from './api'
import i18n from './i18n'
vi.mock('react-virtuoso', () => ({ Virtuoso: ({ data, itemContent }: { data: unknown[]; itemContent: (i: number, row: unknown) => ReactNode }) => <div>{data.map((row, i) => <div key={i}>{itemContent(i, row)}</div>)}</div> }))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
it('shows skill source only from a successful actual load result', async () => {
  await i18n.changeLanguage('en')
  const item = (id: number, kind: TimelineItem['event']['kind'], payload: Record<string, unknown>): TimelineItem => ({ event: { id, kind, payload, agent_id: 'a1', project_id: 'p1', stage_run_id: null, created_at: '' }, message: null })
  useUiStore.setState({ timeline: [
    item(1, 'tool_called', { tool: 'load_skill', action_id: 's1', input: { name: 'ui-ux-pro-max' } }),
    item(2, 'tool_result', { tool: 'load_skill', action_id: 's1', ok: true, result: { output: { name: 'ui-ux-pro-max', source: '/skills/ui-ux-pro-max/SKILL.md', status: 'loaded' } } }),
  ], team: [], pending: [], streams: {}, thinkings: {}, streamDone: {} })
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<Timeline />))
    await act(async () => el.querySelector('.sysrow')!.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    expect(el.textContent).toContain('ui-ux-pro-max')
    expect(el.textContent).toContain('Load skill')
    await act(async () => el.querySelector<HTMLButtonElement>('.tchip-row')!.click())
    expect(el.querySelector('.tchip-detail p')?.textContent).toBe('Loaded · Source: /skills/ui-ux-pro-max/SKILL.md')
  } finally { await act(async () => root.unmount()) }
})
