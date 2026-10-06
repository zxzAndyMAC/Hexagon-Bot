import { afterEach, beforeEach, expect, it, vi } from 'vitest'
const host = vi.hoisted(() => {
  Object.defineProperty(window, '__TAURI__', {value: {}, configurable: true})
  return { invoke: vi.fn(), listen: vi.fn() }
})
vi.mock('@tauri-apps/api/core', () => ({invoke: host.invoke, Channel: class { onmessage = () => {} }}))
vi.mock('@tauri-apps/api/event', () => ({ listen: host.listen }))
import { api, onToolOutput, onTurnDelta, type ToolOutputDelta, type TurnDelta } from './api'
import { useUiStore } from './store'

beforeEach(() => {
  host.invoke.mockReset()
  useUiStore.setState({projectRoot:'/old',projectGeneration:19,timeline:[],timelineFacts:null})
})
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers() })

// Benchmark I1: p1/a0/seq are reused in different project sessions. The host
// event token, including same-root generations, must survive queued delivery.
it('drops queued old-project model deltas and accepts the new subscriber only', async () => {
  vi.useFakeTimers()
  let deliver!: (event: { payload: unknown }) => void
  host.listen.mockImplementation(async (_name: string, callback: typeof deliver) => { deliver = callback; return () => {} })
  const received = vi.fn()
  const unlisten = await onTurnDelta(received)
  const delta: TurnDelta = { agent_id: 'a0', stage_run_id: null, call: 1, text: 'old', thinking: '', reset: false, done: false, waiting: false }
  deliver({ payload: { identity: { project_root: '/old', generation: 19 }, delta } })
  useUiStore.setState({ projectRoot: '/old', projectGeneration: 20 })
  vi.advanceTimersByTime(50)
  expect(received).not.toHaveBeenCalled()
  // Even late arrival after the boundary must not reach a freshly subscribed B.
  unlisten()
  const stopNew = await onTurnDelta(received)
  deliver({ payload: { identity: { project_root: '/old', generation: 19 }, delta } })
  deliver({ payload: { identity: { project_root: '/old', generation: 20 }, delta: { ...delta, text: 'new' } } })
  vi.advanceTimersByTime(50)
  expect(received).toHaveBeenCalledExactlyOnceWith({ ...delta, text: 'new' })
  stopNew()
})

it('rejects stale or unscoped tool output even when agent and seq match', async () => {
  let deliver!: (event: { payload: unknown }) => void
  host.listen.mockImplementation(async (_name: string, callback: typeof deliver) => { deliver = callback; return () => {} })
  const received = vi.fn()
  const stop = await onToolOutput(received)
  const delta: ToolOutputDelta = { agent_id: 'a0', seq: 'r1:i0', stream: 'stdout', text: 'old' }
  deliver({ payload: delta })
  deliver({ payload: { identity: { project_root: '/other', generation: 19 }, delta } })
  deliver({ payload: { identity: { project_root: '/old', generation: 18 }, delta } })
  expect(received).not.toHaveBeenCalled()
  deliver({ payload: { identity: { project_root: '/old', generation: 19 }, delta } })
  expect(received).toHaveBeenCalledExactlyOnceWith(delta)
  useUiStore.setState({ projectGeneration: 20 })
  deliver({ payload: { identity: { project_root: '/old', generation: 19 }, delta } })
  expect(received).toHaveBeenCalledTimes(1)
  stop()
})

it('binds a delayed decision to the host project incarnation at invocation', async () => {
  useUiStore.setState({ projectRoot: '/old', projectGeneration: 19 })
  let finish!: () => void
  host.invoke.mockImplementation(() => new Promise<void>(yes => { finish = yes }))
  const decision = api.answerPermission('q1', true)
  useUiStore.setState({ projectRoot: '/new', projectGeneration: 20 })
  expect(host.invoke).toHaveBeenCalledWith('answer_permission', expect.objectContaining({
    questionId: 'q1', allow: true, _project: { project_root: '/old', generation: 19 },
  }))
  finish()
  await decision
})

it('carries the old host incarnation when close invalidates the visible project', async () => {
  useUiStore.setState({ projectRoot: '/old', projectGeneration: 19 })
  host.invoke.mockResolvedValue(undefined)
  await api.closeProject()
  expect(host.invoke).toHaveBeenCalledWith('close_project', { _project: { project_root: '/old', generation: 19 } })
})

// Issue15/14: waiting for the host status poll retained old-project pixels.
// Every supported project entry invalidates identity before the IPC can await.
it.each(['open', 'recent', 'create', 'close'] as const)('%s invalidates identity synchronously and commits only the canonical host root',async action=>{
  let finish!:()=>void
  host.invoke.mockImplementation((command:string)=>command==='project_identity'
    ? Promise.resolve({project_root:'/canonical/new',generation:20})
    : new Promise<void>(yes=>{finish=yes}))
  const before=useUiStore.getState().projectEpoch
  const result=action==='open' ? api.openProject('/new','same name',[])
    : action==='recent' ? api.openRecent('/new')
    : action==='create' ? api.createProject({dir:'/new',name:'same name',roles:[],initGit:false})
    : api.closeProject()
  expect(useUiStore.getState().projectEpoch).toBe(before+1)
  expect(useUiStore.getState().projectRoot).toBeNull()
  finish();await result
  expect(useUiStore.getState().projectRoot).toBe(action==='close'?null:'/canonical/new')
})
it('failed switch restores the actual old root with a new epoch, never the old window',async()=>{
  useUiStore.setState({timeline:[{event:{id:42,project_id:'p1',stage_run_id:null,agent_id:null,kind:'system',payload:{},created_at:''},message:null}]})
  host.invoke.mockImplementation((command:string)=>command==='project_identity'
    ? Promise.resolve({project_root:'/old',generation:19}) : Promise.reject(new Error('cannot open')))
  const before=useUiStore.getState().projectEpoch
  await expect(api.openRecent('/missing')).rejects.toThrow('cannot open')
  expect(useUiStore.getState().projectEpoch).toBe(before+1)
  expect(useUiStore.getState().projectRoot).toBe('/old')
  expect(useUiStore.getState().timeline).toEqual([])
})
it('late identity discovery from an earlier switch cannot replace the latest project',async()=>{
  let resolveFirst!:(value:unknown)=>void
  host.invoke.mockResolvedValue(undefined)
    .mockImplementationOnce(()=>new Promise(yes=>{resolveFirst=yes}))
  const first=api.openRecent('/first')
  host.invoke.mockImplementation((command:string)=>command==='project_identity' ? Promise.resolve({project_root:'/second',generation:21}) : Promise.resolve())
  await api.openRecent('/second')
  resolveFirst(undefined);await first
  expect(useUiStore.getState().projectRoot).toBe('/second')
})
it('full-history export rejects a project switch during a later page',async()=>{
  let finish!:(value:unknown)=>void
  let ready!:()=>void
  const secondPage=new Promise<void>(yes=>{ready=yes})
  host.invoke.mockResolvedValueOnce(Array.from({length:500},(_,i)=>({event:{id:i+1,kind:'system'},message:null})))
    .mockImplementationOnce(()=>new Promise(yes=>{finish=yes;ready()}))
  const exported=api.exportTimelineItems()
  const assertion=expect(exported).rejects.toThrow('Project changed')
  await secondPage
  useUiStore.getState().beginProjectSwitch()
  finish([{event:{id:501,kind:'system'},message:null}])
  await assertion
})

// Benchmark I1/2026-10-06: timeline invalidation alone left old pending cards,
// model labels and file tabs available in another project (all use project p1).
it('project replacement clears every actionable old card and file tab', () => {
  const prior = useUiStore.getState()
  useUiStore.setState({ pendingDialogOpen: true, dismissedPendingKeys: ['q:old'],
    tabs: [{id:'timeline',kind:'timeline',title:''},{id:'file:README.md',kind:'file',title:'README.md',path:'README.md'}],
    activeTab:'file:README.md', projectName:'old project', splitOpen:true })
  useUiStore.getState().beginProjectSwitch()
  const current=useUiStore.getState()
  expect(current.pendingDialogOpen).toBe(false)
  expect(current.dismissedPendingKeys).toEqual([])
  expect(current.tabs.map(tab=>tab.id)).toEqual(['timeline'])
  expect(current.activeTab).toBe('timeline')
  expect(current.projectName).toBe('')
  expect(current.splitOpen).toBe(false)
  useUiStore.setState(prior, true)
})
