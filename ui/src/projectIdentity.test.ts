import { afterEach, beforeEach, expect, it, vi } from 'vitest'
const host = vi.hoisted(() => {
  Object.defineProperty(window, '__TAURI__', {value: {}, configurable: true})
  return { invoke: vi.fn() }
})
vi.mock('@tauri-apps/api/core', () => ({invoke: host.invoke, Channel: class { onmessage = () => {} }}))
import { api } from './api'
import { useUiStore } from './store'

beforeEach(() => {
  host.invoke.mockReset()
  useUiStore.setState({projectRoot:'/old',timeline:[],timelineFacts:null})
})
afterEach(() => vi.restoreAllMocks())

// Issue15/14: waiting for the host status poll retained old-project pixels.
// Every supported project entry invalidates identity before the IPC can await.
it.each(['open', 'recent', 'create', 'close'] as const)('%s invalidates identity synchronously and commits only the canonical host root',async action=>{
  let finish!:()=>void
  host.invoke.mockImplementation((command:string)=>command==='desktop_status'
    ? Promise.resolve({project_root:'/canonical/new'})
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
  host.invoke.mockImplementation((command:string)=>command==='desktop_status'
    ? Promise.resolve({project_root:'/old'}) : Promise.reject(new Error('cannot open')))
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
  host.invoke.mockImplementation((command:string)=>command==='desktop_status' ? Promise.resolve({project_root:'/second'}) : Promise.resolve())
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
