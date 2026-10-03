import { useEffect, useRef, useState } from 'react'
import { api, errText } from './api'
import { useUiStore } from './store'
import type { TimelineNode } from './gen/TimelineNode'

export function useTimelineNodes() {
  const root = useUiStore(state => state.projectRoot)
  const epoch = useUiStore(state => state.projectEpoch)
  const watermark = useUiStore(state => state.timelineFacts?.latest_event_id ?? 0)
  const [state, setState] = useState<{ epoch: number; nodes: TimelineNode[]; loading: boolean; error: string | null }>({ epoch, nodes: [], loading: true, error: null })
  const cursor = useRef({ epoch, value: 0 })
  useEffect(() => {
    let disposed = false
    if (cursor.current.epoch !== epoch) cursor.current = { epoch, value: 0 }
    if (!root) return
    void api.timelineNodes({ expected_project_root: root, after_event_id: cursor.current.value || null }).then(page => {
      if (disposed || page.project_root !== root || useUiStore.getState().projectEpoch !== epoch) return
      cursor.current.value = Math.max(cursor.current.value, page.watermark)
      setState(previous => ({ epoch, loading: false, error: null,
        nodes: [...new Map([...(previous.epoch === epoch ? previous.nodes : []), ...page.nodes].map(node => [node.event_id, node])).values()].sort((a, b) => a.event_id - b.event_id) }))
    }).catch(error => { if (!disposed) setState(previous => ({ ...previous, loading: false, error: errText(error) })) })
    return () => { disposed = true }
  }, [root, epoch, watermark])
  return state.epoch === epoch && root ? state : { nodes: [], loading: !!root, error: null }
}
