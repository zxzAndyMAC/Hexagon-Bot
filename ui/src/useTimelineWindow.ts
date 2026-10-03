import { pairToolCalls, toolOutcome } from './agentSteps'
import { useCallback, useEffect, useMemo, useRef, useSyncExternalStore } from 'react'
import { api, errText } from './api'
import { useUiStore } from './store'
import type { TimelineWindowPage } from './gen/TimelineWindowPage'
import type { TimelineWindowCursor } from './gen/TimelineWindowCursor'
import type { TimelineFilter } from './gen/TimelineFilter'
import type { TimelineItem } from './gen/TimelineItem'

const PAGE_SIZE = 100
export type TimelineWindowState = {
  page: TimelineWindowPage | null
  loading: boolean
  error: string | null
  olderLoading: boolean
  olderError: string | null
  tailError: string | null
  generation: number
  revision: number
  change: 'replace' | 'prepend' | 'append' | 'metadata'
}
type Entry = {
  root: string; epoch: number; filter: TimelineFilter; agentId?: string
  value: TimelineWindowState; listeners: Set<() => void>
  replace: number; older?: Promise<void>; tail?: Promise<void>; metadata?: Promise<void>; metadataOffset: number; metadataWatermark: number; mode: 'latest' | 'around'
  retryCursor: TimelineWindowCursor; initialized: boolean
}
let nextGeneration = 0
const entries = new Map<string, Entry>()
const idle: TimelineWindowState = { page: null, loading: false, error: null, olderLoading: false, olderError: null, tailError: null, generation: 0, revision: 0, change: 'replace' }
function emit(entry: Entry, update: Partial<TimelineWindowState>) {
  entry.value = { ...entry.value, ...update }
  for (const listener of entry.listeners) listener()
}
function current(entry: Entry, generation: number) {
  const state = useUiStore.getState()
  return state.projectEpoch === entry.epoch && state.projectRoot === entry.root && entry.replace === generation
}
function unique(items: TimelineItem[]) {
  return [...new Map(items.map(item => [item.event.id, item])).values()].sort((a, b) => a.event.id - b.event.id)
}
function merge(previous: TimelineWindowPage, next: TimelineWindowPage, before: boolean): TimelineWindowPage {
  return { ...next,
    items: unique([...previous.items, ...next.items]),
    boundary_pairs: unique([...previous.boundary_pairs, ...next.boundary_pairs]),
    turn_windows: [...new Map([...previous.turn_windows, ...next.turn_windows].map(turn => [turn.start_id, turn])).values()],
    chapter_base: before ? next.chapter_base : previous.chapter_base,
    has_before: before ? next.has_before : previous.has_before,
    has_after: before ? previous.has_after : next.has_after,
    watermark: Math.max(previous.watermark, next.watermark),
    target_event_id: previous.target_event_id,
    steered_message_ids: [...new Set([...previous.steered_message_ids, ...next.steered_message_ids])],
  }
}
async function request(entry: Entry, cursor: TimelineWindowCursor) {
  const replacing = cursor.kind === 'latest' || cursor.kind === 'around'
  const before = cursor.kind === 'before'
  if (replacing) {
    entry.replace++
    entry.mode = cursor.kind === 'around' ? 'around' : 'latest'
    entry.retryCursor = cursor
    entry.metadataWatermark = 0
    emit(entry, { loading: true, error: null, olderLoading: false, olderError: null, tailError: null,
      generation: ++nextGeneration, change: 'replace' })
  } else if (before) emit(entry, { olderLoading: true, olderError: null })
  const generation = entry.replace
  const edge = before ? entry.value.page?.items[0]?.event.id : entry.value.page?.items.at(-1)?.event.id
  try {
    const page = await api.timelineWindow({ expected_project_root: entry.root, filter: entry.filter,
      agent_id: entry.agentId ?? null, cursor, limit: PAGE_SIZE })
    if (!current(entry, generation) || page.project_root !== entry.root) return
    const previous = entry.value.page
    if (!replacing && previous && edge !== (before ? previous.items[0]?.event.id : previous.items.at(-1)?.event.id)) return
    const next = replacing || !previous ? page : merge(previous, page, before)
    emit(entry, { page: next,
      ...(replacing ? { loading:false,error:null,olderLoading:false,olderError:null,tailError:null }
        : before ? { olderLoading:false,olderError:null } : { tailError:null }),
      revision: entry.value.revision + 1, change: replacing ? 'replace' : before ? 'prepend' : 'append' })
    // Compatibility view only: facts and other consumers must not scan this array.
    if (!entry.agentId) useUiStore.setState({ timeline: next.items, timelineCaughtUp: true })
  } catch (error) {
    if (!current(entry, generation)) return
    entry.retryCursor = cursor
    emit(entry, replacing ? { loading: false, error: errText(error) }
      : before ? { olderLoading: false, olderError: errText(error) } : { tailError: errText(error) })
  }
}
function getEntry(root: string, epoch: number, filter: TimelineFilter, agentId?: string) {
  // Project transitions invalidate all cache entries, including hidden tabs.
  for (const [key, value] of entries) if (value.epoch !== epoch || value.root !== root) entries.delete(key)
  const key = JSON.stringify([root, epoch, filter, agentId ?? null])
  let entry = entries.get(key)
  if (!entry) {
    entry = { root, epoch, filter, agentId, value: { ...idle, loading: true }, listeners: new Set(),
      replace: 0, metadataOffset: 0, metadataWatermark: 0, mode: 'latest', retryCursor: { kind: 'latest' }, initialized: false }
    entries.set(key, entry)
  }
  return entry
}
async function refreshEntryTail(entry: Entry | null) {
  if (!entry || entry.tail || entry.value.loading || entry.mode === 'around' || !entry.value.page) return
  const after = entry.value.page.items.at(-1)?.event.id
  if (after == null) return request(entry, { kind: 'latest' })
  entry.tail = (async () => {
    let edge = after
    while (current(entry, entry.replace)) {
      const generation = entry.replace
      const revision = entry.value.revision
      await request(entry, { kind: 'after', event_id: edge })
      if (!current(entry, generation) || entry.value.revision === revision || !entry.value.page?.has_after) break
      const next = entry.value.page.items.at(-1)?.event.id
      if (next == null || next <= edge) break
      edge = next
    }
  })()
  try { await entry.tail } finally { entry.tail = undefined }
}
async function refreshEntryMetadata(entry: Entry | null, watermark: number) {
  if (!entry || entry.metadata || entry.value.loading || !entry.value.page || watermark <= entry.metadataWatermark) return
  const page = entry.value.page
  const support = unique([...page.items, ...page.boundary_pairs])
  const primary = new Set(page.items.map(item => item.event.id))
  const ids = new Set<number>()
  for (const call of pairToolCalls(support)) if (!call.result || toolOutcome(call.result) === 'unknown') {
    if (primary.has(call.called.event.id)) ids.add(call.called.event.id)
    else if (call.result && primary.has(call.result.event.id)) ids.add(call.result.event.id)
  }
  for (const turn of page.turn_windows) if (turn.end_id == null) {
    const item = page.items.find(item => item.event.agent_id === turn.agent_id && item.event.id >= turn.start_id)
    if (item) ids.add(item.event.id)
  }
  if (!ids.size) return
  const all = [...ids]
  const start = entry.metadataOffset % all.length
  const watched = [...all.slice(start), ...all.slice(0, start)].slice(0, 500)
  entry.metadataOffset = (start + watched.length) % all.length
  const generation = entry.replace
  entry.metadata = (async () => {
    try {
      const metadata = await api.timelineWindowMetadata({expected_project_root:entry.root,event_ids:watched})
      if (!current(entry, generation) || metadata.project_root !== entry.root || !entry.value.page) return
      const previous = entry.value.page
      if (metadata.watermark < previous.watermark) return
      entry.metadataWatermark = metadata.watermark
      emit(entry, {page: {...previous,
        boundary_pairs: unique([...previous.boundary_pairs, ...metadata.boundary_pairs]),
        turn_windows: [...new Map([...previous.turn_windows, ...metadata.turn_windows].map(turn => [turn.start_id, turn])).values()],
        steered_message_ids: [...new Set([...previous.steered_message_ids,...metadata.steered_message_ids])],
      }, change:'metadata',revision:entry.value.revision+1})
    } catch { /* Preserve current evidence; the next facts poll retries metadata. */ }
  })()
  try { await entry.metadata } finally { entry.metadata = undefined }
}
async function loadEntryOlder(entry: Entry | null) {
  if (!entry || entry.older || entry.value.loading || !entry.value.page?.has_before) return
  const first = entry.value.page.items[0]?.event.id
  if (first == null) return
  entry.older = request(entry, { kind: 'before', event_id: first })
  try { await entry.older } finally { entry.older = undefined }
}
async function retryEntry(entry: Entry | null) {
  if (!entry) return
  const cursor = entry.retryCursor
  if (cursor.kind === 'before' || cursor.kind === 'after') {
    const lane = cursor.kind === 'before' ? 'older' : 'tail'
    if (entry[lane]) return entry[lane]
    const pending = request(entry, cursor)
    entry[lane] = pending
    try { await pending } finally { if (entry[lane] === pending) entry[lane] = undefined }
    if (cursor.kind === 'after' && !entry.value.tailError && entry.value.page?.has_after) await refreshEntryTail(entry)
  } else await request(entry, cursor)
}
function initializeEntry(entry: Entry) {
  entry.initialized = true
  void request(entry, { kind: 'latest' })
}

export function useTimelineWindow(filter: TimelineFilter = 'all', agentId?: string) {
  const root = useUiStore(state => state.projectRoot)
  const epoch = useUiStore(state => state.projectEpoch)
  const facts = useUiStore(state => state.timelineFacts)
  const fallback = useUiStore(state => state.timeline)
  const entry = root ? getEntry(root, epoch, filter, agentId) : null
  const state = useSyncExternalStore(listener => {
    entry?.listeners.add(listener)
    return () => { entry?.listeners.delete(listener) }
  }, () => entry?.value ?? idle)
  const previousFilter = useRef(filter)
  useEffect(() => {
    const changed = previousFilter.current !== filter
    previousFilter.current = filter
    if (!entry || (entry.initialized && !changed)) return
    initializeEntry(entry)
  }, [entry, filter])
  const refreshTail = useCallback(() => refreshEntryTail(entry), [entry])
  const refreshMetadata = useCallback(() => refreshEntryMetadata(entry, facts?.latest_event_id ?? 0), [entry, facts?.latest_event_id])
  useEffect(() => {
    if (!entry || !facts || facts.project_root !== entry.root) return
    const page = entry.value.page
    if (!page) return
    const steered = [...new Set([...page.steered_message_ids, ...facts.steered_message_ids])]
    if (steered.length !== page.steered_message_ids.length) emit(entry, {
      page: { ...page, steered_message_ids: steered }, change: 'metadata', revision: entry.value.revision + 1,
    })
    if (facts.latest_event_id > page.watermark) { void refreshTail(); void refreshMetadata() }
  }, [entry, facts, refreshTail, refreshMetadata])
  // Rootless fixtures retain their existing event seam. A real transition never
  // falls back: epoch>0/root=null means deliberately hidden pending identity.
  const page = useMemo(() => !entry && epoch === 0 && fallback.length ? {
    project_root: '', items: agentId ? fallback.filter(item => item.event.agent_id === agentId) : fallback,
    boundary_pairs: [], turn_windows: [], chapter_base: 0, has_before: false, has_after: false,
    watermark: fallback.at(-1)?.event.id ?? 0, target_event_id: null, steered_message_ids: [],
  } : state.page, [entry, epoch, fallback, agentId, state.page])
  const actions = useMemo(() => ({
    loadOlder: () => loadEntryOlder(entry),
    loadLatest: async () => { if (entry) await request(entry, { kind: 'latest' }) },
    jumpAround: async (id: number) => { if (entry) await request(entry, { kind: 'around', event_id: id }) },
    retry: () => retryEntry(entry),
    refreshTail,
  }), [entry, refreshTail])
  return { ...state, page, ...actions }
}

/** Only currently loaded, not-yet-injected owner message IDs need receipts. */
export function watchedTimelineMessages(root: string, epoch: number): number[] {
  const ids = new Set<number>()
  for (const entry of entries.values()) {
    if (entry.root !== root || entry.epoch !== epoch || !entry.value.page) continue
    const page = entry.value.page
    for (const item of page.items) if (item.message?.author === 'owner' && !page.steered_message_ids.includes(item.message.id)) ids.add(item.message.id)
  }
  return [...ids].slice(-500)
}
