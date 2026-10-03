import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { api } from './api'
import { useUiStore } from './store'
import { useTimelineWindow } from './useTimelineWindow'
import type { TimelineFilter } from './gen/TimelineFilter'
import type { TimelineWindowPage } from './gen/TimelineWindowPage'
import type { TimelineFacts } from './gen/TimelineFacts'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
let view: ReturnType<typeof useTimelineWindow>
function Probe({ filter = 'all' }: { filter?: TimelineFilter }) { view = useTimelineWindow(filter); return <div>{view.page?.items.map(item => item.event.id).join(',')}</div> }
function page(ids: number[], extra: Partial<TimelineWindowPage> = {}): TimelineWindowPage {
  return { project_root: '/project', items: ids.map(id => ({event: { id, project_id: 'p1', stage_run_id: null, agent_id: null, kind: 'system', payload: {}, created_at: '2026-10-02T00:00:00Z' },message:null})), boundary_pairs: [],turn_windows:[],chapter_base:0,has_before:false,has_after:false,watermark:ids.at(-1)??0,target_event_id:null,steered_message_ids:[],...extra }
}
function facts(id: number): TimelineFacts { return {project_root:'/project',latest_event_id:id,active_turns:[],latest_contexts:[],latest_agent_messages:[],latest_plans:[],settled_tool_streams:[],approval_mode_revision:0,latest_turn_start_id:null,steered_message_ids:[]} }
beforeEach(() => {
  useUiStore.getState().beginProjectSwitch()
  useUiStore.getState().commitProjectRoot('/project',useUiStore.getState().projectEpoch)
  root=createRoot(el)
})
afterEach(async () => { await act(async()=>root.unmount()); vi.restoreAllMocks() })
it('loads bounded latest once and serializes older requests without losing the window on retry',async()=>{
  let fail!: (reason: Error)=>void
  const load=vi.spyOn(api,'timelineWindow').mockResolvedValueOnce(page([90,100],{has_before:true}))
    .mockImplementationOnce(()=>new Promise((_yes,no)=>{fail=no}))
    .mockResolvedValueOnce(page([70,80],{has_after:true}))
  await act(async()=>root.render(<Probe/>))
  expect(load.mock.calls[0][0].cursor).toEqual({kind:'latest'})
  let pending!:Promise<void>
  await act(async()=>{pending=view.loadOlder();void view.loadOlder()})
  expect(load).toHaveBeenCalledTimes(2)
  await act(async()=>{fail(new Error('history unavailable'));await pending})
  expect(view.page?.items.map(item=>item.event.id)).toEqual([90,100])
  expect(view.olderError).toContain('history unavailable')
  await act(async()=>view.retry())
  expect(load.mock.calls[2][0].cursor).toEqual({kind:'before',event_id:90})
  expect(view.page?.items.map(item=>item.event.id)).toEqual([70,80,90,100])
})
it('discards late filter responses and gives each replacement a distinct generation',async()=>{
  let resolve!: (page:TimelineWindowPage)=>void
  vi.spyOn(api,'timelineWindow').mockImplementationOnce(()=>new Promise(yes=>{resolve=yes})).mockResolvedValueOnce(page([8]))
  await act(async()=>root.render(<Probe/>));const original=view.generation
  await act(async()=>root.render(<Probe filter="messages"/>))
  expect(view.generation).toBeGreaterThan(original)
  await act(async()=>resolve(page([1,2])))
  expect(view.page?.items.map(item=>item.event.id)).toEqual([8])
})
it('keeps around windows contiguous when fresh facts arrive',async()=>{
  const load=vi.spyOn(api,'timelineWindow').mockResolvedValueOnce(page([900])).mockResolvedValueOnce(page([40,50],{has_after:true,target_event_id:50}))
  await act(async()=>root.render(<Probe/>))
  await act(async()=>view.jumpAround(50))
  await act(async()=>useUiStore.setState({timelineFacts:facts(1000)}))
  expect(load).toHaveBeenCalledTimes(2)
  expect(view.page?.items.map(item=>item.event.id)).toEqual([40,50])
})
it('drains every contiguous after page even when its watermark is already the global tail',async()=>{
  const load=vi.spyOn(api,'timelineWindow').mockResolvedValueOnce(page([1]))
    .mockResolvedValueOnce(page([2],{has_after:true,watermark:3})).mockResolvedValueOnce(page([3],{watermark:3}))
  await act(async()=>root.render(<Probe/>))
  await act(async()=>useUiStore.setState({timelineFacts:facts(3)}))
  expect(load).toHaveBeenCalledTimes(3)
  expect(view.page?.items.map(item=>item.event.id)).toEqual([1,2,3])
})
it('synchronously hides a previous project and rejects its late latest response',async()=>{
  let resolve!: (page:TimelineWindowPage)=>void
  vi.spyOn(api,'timelineWindow').mockImplementationOnce(()=>new Promise(yes=>{resolve=yes}))
  await act(async()=>root.render(<Probe/>))
  await act(async()=>useUiStore.getState().beginProjectSwitch())
  expect(view.page).toBeNull()
  await act(async()=>resolve(page([999])))
  expect(view.page).toBeNull()
  expect(useUiStore.getState().timeline).toEqual([])
})
it('keeps content and generation visible when tail refresh fails, and retries the same cursor',async()=>{
  const load=vi.spyOn(api,'timelineWindow').mockResolvedValueOnce(page([1])).mockRejectedValueOnce(new Error('tail unavailable')).mockResolvedValueOnce(page([2]))
  await act(async()=>root.render(<Probe/>));const generation=view.generation
  await act(async()=>useUiStore.setState({timelineFacts:facts(2)}))
  expect(view.error).toBeNull();expect(view.tailError).toContain('tail unavailable')
  expect(view.page?.items.map(item=>item.event.id)).toEqual([1]);expect(view.generation).toBe(generation)
  await act(async()=>view.retry())
  expect(load.mock.calls[2][0].cursor).toEqual({kind:'after',event_id:1})
  expect(view.page?.items.map(item=>item.event.id)).toEqual([1,2])
})
it('updates late tool completion metadata in an around window without appending a gap',async()=>{
  const active=page([50],{has_after:true,watermark:100,target_event_id:50})
  active.items[0].event.kind='tool_called';active.items[0].event.agent_id='worker';active.items[0].event.payload={tool:'bash',action_id:'call',seq:'seq'}
  const done=page([120]).items[0];done.event.kind='tool_result';done.event.agent_id='worker';done.event.payload={action_id:'call',ok:true}
  vi.spyOn(api,'timelineWindow').mockResolvedValueOnce(page([100])).mockResolvedValueOnce(active)
  const metadata=vi.spyOn(api,'timelineWindowMetadata').mockResolvedValue({project_root:'/project',watermark:120,boundary_pairs:[done],turn_windows:[],steered_message_ids:[]})
  await act(async()=>root.render(<Probe/>));await act(async()=>view.jumpAround(50))
  await act(async()=>useUiStore.setState({timelineFacts:facts(120)}))
  expect(metadata.mock.calls[0][0].event_ids).toEqual([50])
  expect(view.page?.items.map(item=>item.event.id)).toEqual([50])
  expect(view.page?.boundary_pairs.map(item=>item.event.id)).toEqual([120])
  expect(view.change).toBe('metadata')
})
