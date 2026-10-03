import { act, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { Timeline } from './components/Timeline'
import { api } from './api'
import type { TimelineWindowPage } from './gen/TimelineWindowPage'
import { useUiStore } from './store'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent, scrollerRef }: { data: unknown[]; itemContent: (i: number, row: unknown) => ReactNode; scrollerRef: (node: HTMLElement | null) => void }) =>
    <div data-testid="history-scroller" ref={node => {
      if (node) {
        Object.defineProperty(node,'clientHeight',{configurable:true,value:500})
        node.getBoundingClientRect=()=>new DOMRect(0,0,700,500)
      }
      scrollerRef(node)
    }}>{data.map((row, index) => <div key={index}>{itemContent(index, row)}</div>)}</div>,
}))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
afterEach(() => { vi.restoreAllMocks(); useUiStore.setState({ projectRoot: null, projectEpoch: 0, timeline: [], timelineFacts: null }) })

it('a real project with its initial window pending renders one loading surface without a render loop', async () => {
  // Issue15: a fresh [] fallback changed row identity on every render and
  // recursively reset the virtualizer before the initial IPC could finish.
  vi.spyOn(api, 'timelineWindow').mockImplementation(() => new Promise(() => {}))
  vi.spyOn(api, 'timelineNodes').mockResolvedValue({ project_root: '/window-initial', watermark: 0, nodes: [] })
  useUiStore.setState({ projectRoot: '/window-initial', projectEpoch: 10, timeline: [], timelineFacts: null,
    pending: [], team: [], streams: {}, thinkings: {}, plans: {}, streamDone: {}, stages: [] })
  const el = document.createElement('div'), root = createRoot(el)
  try {
    await act(async () => root.render(<Timeline />))
    expect(el.querySelector('[role="status"]')).not.toBeNull()
    expect(api.timelineWindow).toHaveBeenCalledTimes(1)
    await act(async () => useUiStore.getState().beginProjectSwitch())
    expect(el.querySelector('[role="status"]')).not.toBeNull()
    expect(el.querySelector('[aria-busy="true"]')).not.toBeNull()
  } finally { await act(async () => root.unmount()) }
})

it('loads older history on upward intent without a persistent button, keeping failure retry visible',async()=>{
  const page: TimelineWindowPage={project_root:'/history',items:[{event:{id:50,project_id:'p1',stage_run_id:null,agent_id:null,kind:'system',payload:{kind:'fixture'},created_at:'2026-10-02T00:00:00Z'},message:null}],boundary_pairs:[],turn_windows:[],chapter_base:0,has_before:true,has_after:false,watermark:50,target_event_id:null,steered_message_ids:[]}
  const load=vi.spyOn(api,'timelineWindow').mockResolvedValueOnce(page).mockRejectedValueOnce(new Error('offline')).mockResolvedValue({...page,has_before:false})
  vi.spyOn(api,'timelineNodes').mockResolvedValue({project_root:'/history',watermark:50,nodes:[]})
  useUiStore.setState({projectRoot:'/history',projectEpoch:20,timeline:[],timelineFacts:null,pending:[],team:[],streams:{},thinkings:{},plans:{},streamDone:{},stages:[]})
  const el=document.createElement('div'),root=createRoot(el)
  try {
    await act(async()=>root.render(<Timeline/>))
    await act(async()=>new Promise(resolve=>setTimeout(resolve,250)))
    // Owner 2026-10-02: history remains discoverable through upward scrolling,
    // while a floating Load earlier button should not cover timeline content.
    expect([...el.querySelectorAll('button')].some(button=>button.textContent===i18n.t('timeline.loadOlder'))).toBe(false)
    const scroller=el.querySelector('[data-testid=history-scroller]')!
    await act(async()=>scroller.dispatchEvent(new WheelEvent('wheel',{deltaY:-100,bubbles:true})))
    expect(load).toHaveBeenCalledTimes(2)
    const retry=[...el.querySelectorAll('button')].find(button=>button.textContent===i18n.t('timeline.olderFailed'))!
    expect(retry).toBeTruthy()
    await act(async()=>retry.click())
    expect(load).toHaveBeenCalledTimes(3)
    expect(el.textContent).not.toContain(i18n.t('timeline.olderFailed'))
  } finally {await act(async()=>root.unmount())}
})
