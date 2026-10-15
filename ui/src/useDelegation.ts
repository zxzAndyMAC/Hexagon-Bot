import { useCallback, useEffect } from 'react'
import { create } from 'zustand'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import type { DelegationStatus } from './gen/DelegationStatus'
import type { ManagedRecord } from './gen/ManagedRecord'
import type { ManagedSummary } from './gen/ManagedSummary'

/** Host-persisted delegation state, shared by every mounted view (pill,
 * strip, approval menu, report, timeline rows). Never optimistic: views
 * render only what the host returned, and a response that arrives after a
 * workspace switch (different project root) or a newer read is dropped.
 * ui-redesign 票 08：原先每个挂载点各读一遍（胶囊 + 细条 + 时间线行会成倍发
 * IPC），改成同一份共享读；读取键 = 项目根 + 最新事件 id，没有订阅者时清空。 */
type DelegationState = {
  status: DelegationStatus | null
  processing: PendingQuestion[]
  failed: boolean
  history: ManagedRecord[] | null
  historyFailed: boolean
  /** gap A2/A3：托管时段与预算 / 花费；与 status 同一读取键一起读。读失败不影响胶囊，只是不出这两行。 */
  summary: ManagedSummary | null
}

const useDelegationStore = create<DelegationState>(() => ({
  status: null, processing: [], failed: false, history: null, historyFailed: false, summary: null,
}))

let generation = 0
let subscribers = 0
let loadedKey: string | null = null
let historyKey: string | null = null
let historySubscribers = 0

function load(key: string | null) {
  if (key !== null && key === loadedKey) return
  loadedKey = key
  const current = ++generation
  const summary = api.managedSummary().catch(() => null)
  void Promise.all([api.projectDelegation(), api.managedProcessing(), summary]).then(([status, held, facts]) => {
    if (current !== generation || subscribers === 0) return
    useDelegationStore.setState({ status, processing: held, failed: false, summary: facts })
  }).catch(() => {
    if (current === generation && subscribers > 0) useDelegationStore.setState({ failed: true })
  })
}

let reloadQueued = false
/** A view mounting later (e.g. the pill moving into the full-width canvas bar)
 * re-reads once instead of trusting the cached read; mounts in the same tick
 * coalesce into one read. */
function queueReload() {
  if (reloadQueued) return
  reloadQueued = true
  queueMicrotask(() => { reloadQueued = false; if (subscribers > 0) load(null) })
}

function loadHistory(key: string) {
  if (key === historyKey) return
  historyKey = key
  void api.managedHistory().then(history => {
    if (historyKey === key && historySubscribers > 0) useDelegationStore.setState({ history, historyFailed: false })
  }).catch(() => {
    if (historyKey === key && historySubscribers > 0) useDelegationStore.setState({ historyFailed: true })
  })
}

function useLatestKey() {
  const root = useUiStore(s => s.projectRoot)
  const latest = useUiStore(s => s.timelineFacts?.latest_event_id ?? 0)
  return { root, key: `${root ?? ''}:${latest}` }
}

export function useDelegation() {
  const { root, key } = useLatestKey()
  const status = useDelegationStore(s => s.status)
  const processingItems = useDelegationStore(s => s.processing)
  const failed = useDelegationStore(s => s.failed)
  const summary = useDelegationStore(s => s.summary)
  useEffect(() => {
    if (subscribers > 0) queueReload()
    subscribers += 1
    return () => {
      subscribers -= 1
      if (subscribers === 0) {
        generation += 1; loadedKey = null
        useDelegationStore.setState({ status: null, processing: [], failed: false, summary: null })
      }
    }
  }, [])
  useEffect(() => { load(key) }, [key])
  const reload = useCallback(() => load(null), [])
  /** Apply a host write; the result only lands for the same project root. */
  const apply = useCallback(async (write: (root: string) => Promise<DelegationStatus>) => {
    if (!root) throw new Error('Workspace identity unavailable')
    const current = ++generation
    const saved = await write(root)
    if (current !== generation || subscribers === 0) return
    useDelegationStore.setState({ status: saved })
    load(null)
  }, [root])
  return { status, root, processing: processingItems.length, processingItems, failed, summary, apply, reload }
}

/** Managed decision records (newest first), shared by the report and the
 * timeline's managed rows. `null` until the first read lands. */
export function useManagedHistory() {
  const { key } = useLatestKey()
  const history = useDelegationStore(s => s.history)
  const failed = useDelegationStore(s => s.historyFailed)
  useEffect(() => {
    historySubscribers += 1
    return () => {
      historySubscribers -= 1
      if (historySubscribers === 0) {
        historyKey = null
        useDelegationStore.setState({ history: null, historyFailed: false })
      }
    }
  }, [])
  useEffect(() => { loadHistory(key) }, [key])
  return { history, failed }
}
