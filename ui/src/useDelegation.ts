import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from './api'
import { useUiStore } from './store'
import type { DelegationStatus } from './gen/DelegationStatus'

/** Host-persisted delegation state for one mounted view. Never optimistic:
 * callers render only what the host returned, and a response that arrives
 * after a workspace switch (different project root) is dropped. */
export function useDelegation() {
  const [status, setStatus] = useState<DelegationStatus | null>(null)
  // The host-confirmed workspace identity, not a second approval-mode read.
  const root = useUiStore(s => s.projectRoot)
  const [processing, setProcessing] = useState(0)
  const [failed, setFailed] = useState(false)
  const latest = useUiStore(s => s.timelineFacts?.latest_event_id ?? 0)
  const generation = useRef(0)
  const mounted = useRef(true)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  const load = useCallback(() => {
    const current = ++generation.current
    void Promise.all([api.projectDelegation(), api.managedProcessing()]).then(([value, held]) => {
      if (!mounted.current || current !== generation.current) return
      setStatus(value); setProcessing(held.length); setFailed(false)
    }).catch(() => { if (mounted.current && current === generation.current) setFailed(true) })
  }, [])
  useEffect(() => { load() }, [latest, root, load])
  /** Apply a host write; the result only lands for the same project root. */
  const apply = useCallback(async (write: (root: string) => Promise<DelegationStatus>) => {
    if (!root) throw new Error('Workspace identity unavailable')
    const expected = root
    const current = ++generation.current
    const saved = await write(expected)
    if (!mounted.current || current !== generation.current) return
    setStatus(saved)
    load()
  }, [root, load])
  return { status, root, processing, failed, apply, reload: load }
}
