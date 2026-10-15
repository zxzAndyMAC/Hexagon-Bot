import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { api } from './api'
import type { TimelineNode } from './gen/TimelineNode'
import type { TimelineFacts } from './gen/TimelineFacts'
import { useUiStore } from './store'
import { useTimelineNodes } from './useTimelineNodes'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
afterEach(() => { vi.restoreAllMocks(); useUiStore.setState({ projectRoot: null, projectEpoch: 0, timelineFacts: null }) })

const node = (event_id: number, cls: TimelineNode['class']): TimelineNode =>
  ({ event_id, kind: cls === 'failed' ? 'turn_failed' : 'stage_started', class: cls, label: String(event_id), summary: null, agent_id: null, created_at: '' })

// gap A1（2026-10-15）：增量节点读的 retired 撤掉此前交出、现已恢复的失败节点。
it('merges incremental node reads and drops failures the host retired', async () => {
  const read = vi.spyOn(api, 'timelineNodes')
    .mockResolvedValueOnce({ project_root: '/n', watermark: 5, nodes: [node(2, 'milestone'), node(4, 'failed')], retired: [] })
    .mockResolvedValueOnce({ project_root: '/n', watermark: 8, nodes: [node(7, 'owner')], retired: [4] })
  useUiStore.setState({ projectRoot: '/n', projectEpoch: 3, timelineFacts: { latest_event_id: 5 } as TimelineFacts })
  let seen: ReturnType<typeof useTimelineNodes> | null = null
  const Probe = () => { seen = useTimelineNodes(); return null }
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Probe />))
    expect(seen!.nodes.map(n => n.event_id)).toEqual([2, 4])
    await act(async () => useUiStore.setState({ timelineFacts: { latest_event_id: 8 } as TimelineFacts }))
    expect(read).toHaveBeenLastCalledWith({ expected_project_root: '/n', after_event_id: 5 })
    expect(seen!.nodes.map(n => [n.event_id, n.class])).toEqual([[2, 'milestone'], [7, 'owner']])
    expect(seen!.watermark).toBe(8)
  } finally { await act(async () => root.unmount()) }
})
