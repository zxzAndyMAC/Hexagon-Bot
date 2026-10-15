import { useEffect, useRef, useState } from 'react'
import { api, errText } from './api'
import { useUiStore } from './store'
import type { TimelineNode } from './gen/TimelineNode'

export function useTimelineNodes() {
  const root = useUiStore(state => state.projectRoot)
  const epoch = useUiStore(state => state.projectEpoch)
  const watermark = useUiStore(state => state.timelineFacts?.latest_event_id ?? 0)
  const [state, setState] = useState<{ epoch: number; nodes: TimelineNode[]; watermark: number; loading: boolean; error: string | null }>({ epoch, nodes: [], watermark: 0, loading: true, error: null })
  const cursor = useRef({ epoch, value: 0 })
  useEffect(() => {
    let disposed = false
    if (cursor.current.epoch !== epoch) cursor.current = { epoch, value: 0 }
    if (!root) return
    void api.timelineNodes({ expected_project_root: root, after_event_id: cursor.current.value || null }).then(page => {
      if (disposed || page.project_root !== root || useUiStore.getState().projectEpoch !== epoch) return
      cursor.current.value = Math.max(cursor.current.value, page.watermark)
      // gap A1：增量读里此前交出的失败节点若已恢复，宿主在 retired 里点名，合并时撤掉。
      const retired = new Set(page.retired)
      setState(previous => ({ epoch, loading: false, error: null, watermark: Math.max(previous.epoch === epoch ? previous.watermark : 0, page.watermark),
        nodes: [...new Map([...(previous.epoch === epoch ? previous.nodes : []), ...page.nodes].filter(node => !retired.has(node.event_id)).map(node => [node.event_id, node])).values()].sort((a, b) => a.event_id - b.event_id) }))
    }).catch(error => { if (!disposed) setState(previous => ({ ...previous, loading: false, error: errText(error) })) })
    return () => { disposed = true }
  }, [root, epoch, watermark])
  return state.epoch === epoch && root ? state : { nodes: [], watermark: 0, loading: !!root, error: null }
}
