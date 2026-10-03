import { useEffect, useRef, useState } from 'react'
import { api, errText, type StageEvidence } from './api'
import { useUiStore } from './store'

/** Live acceptance #15: serialize worktree reads and discard superseded requests.
 * Keep invalidated evidence hidden: old checks must never look current. */
export function useStageEvidence(identity?: string) {
  const revision = useUiStore((s) => s.evidenceRevision)
  const eventId = useUiStore((s) => s.timelineFacts?.latest_event_id)
  const activeRun = useUiStore((s) => s.stages.find((r) => r.state === 'active' || r.state === 'waiting_stamp')?.run_id)
  const pending = useRef<Promise<void> | undefined>(undefined)
  const [evidence, setEvidence] = useState<StageEvidence | null>(null)
  const [error, setError] = useState('')
  useEffect(() => {
    let disposed = false
    let request = 0
    const refresh = () => {
      const current = ++request
      setEvidence(null)
      setError('')
      const previous = pending.current
      pending.current = (async () => {
        await previous
        if (disposed || current !== request) return
        try {
          const next = await api.stageEvidence()
          if (!disposed && current === request) setEvidence(next)
        } catch (e) {
          if (!disposed && current === request) setError(errText(e))
        }
      })()
    }
    refresh()
    window.addEventListener('focus', refresh)
    return () => { disposed = true; window.removeEventListener('focus', refresh) }
  }, [identity, activeRun, eventId, revision])
  return { evidence, error }
}
