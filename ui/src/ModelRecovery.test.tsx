import { act, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import './i18n'
import { api, type PendingQuestion, type TimelineItem } from './api'
import type { ModelRequestRecovery } from './gen/ModelRequestRecovery'
import { PendingCard } from './components/PendingCards'
import { ModelRecoveryStatus } from './components/ModelRecoveryStatus'
import { Timeline } from './components/Timeline'
import { approveQuestion } from './decisions'
import { useUiStore } from './store'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({data,itemContent}: {data: unknown[];itemContent: (i:number,row:unknown)=>ReactNode}) => <div>{data.map((r,i)=><div key={i}>{itemContent(i,r)}</div>)}</div>,
}))
;(globalThis as Record<string,unknown>).IS_REACT_ACT_ENVIRONMENT=true
const view: ModelRequestRecovery = {recovery_id:'q1',purpose:'pm_route',state:'failed',project_root:'/project',agent_id:'pm',stage_run_id:null,target_role:'Product',history:[{code:'transport',model:null,model_slot:'decision',request_id:'request1',http_status:null,message:'unexpected EOF'}],can_retry:true,partial_text:null,unfinished:false,retry_deadline_ms:null}
const payload={...view,sub:'model_request'}
const card: PendingQuestion={id:'q1',kind:'recovery',agent_id:'pm',payload,state:'queued'}
afterEach(()=>vi.restoreAllMocks())
it('shows the failed dispatch and retries the original item through the same shortcut action',async()=>{
 const retry=vi.spyOn(api,'retryModelRequest').mockResolvedValue(view)
 const generic=vi.spyOn(api,'recoverRun').mockResolvedValue()
 useUiStore.setState({pending:[card],team:[],toasts:[],modalScope:'workbench'})
 const el=document.createElement('div');const root=createRoot(el)
 try {
  await act(async()=>root.render(<PendingCard q={card} top />))
  expect(el.textContent).toContain('Dispatch failed')
  expect(el.textContent).toContain('Product')
  const button=Array.from(el.querySelectorAll('button')).find(b=>b.textContent?.includes('Retry original request'))!
  expect(button).toBeTruthy();expect(button.title).toBeTruthy()
  await act(async()=>button.click())
  await approveQuestion(card)
  expect(retry).toHaveBeenNthCalledWith(1,'q1','/project')
  expect(retry).toHaveBeenNthCalledWith(2,'q1','/project')
  expect(generic).not.toHaveBeenCalled()
 }finally {await act(async()=>root.unmount())}
})
it('shows one recovered timeline item with its failure history outside system folds',async()=>{
 const ev=(id:number,state:string):TimelineItem=>({event:{id,kind:'system',agent_id:'pm',stage_run_id:null,project_id:'p1',created_at:'',payload:{...payload,kind:state==='failed'?'model_request_failed':'model_request_recovered',recovery_id:'q1',state}},message:null})
 useUiStore.setState({timeline:[ev(1,'failed'),ev(2,'failed'),ev(3,'recovered')],pending:[],team:[],streams:{},thinkings:{},streamDone:{}})
 const el=document.createElement('div');const root=createRoot(el)
 try{
  await act(async()=>root.render(<Timeline />))
  expect(el.querySelectorAll('[data-model-recovery="q1"]')).toHaveLength(1)
  expect(el.textContent).toContain('Recovered')
  const details=el.querySelector('details')!
  expect(details.textContent).toContain('request1')
  expect(el.querySelector('button[title*="Retry original request"]')).toBeNull()
 }finally{await act(async()=>root.unmount())}
})

it('offers direct settings entry for a non-retryable credential failure', async () => {
 const el=document.createElement('div'); const root=createRoot(el)
 const credential={...payload,history:[{...payload.history[0],code:'missing_credential'}]}
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={credential} />))
  expect(el.textContent).toContain('Open model settings')
  expect(el.textContent).not.toContain('Open usage')
 } finally { await act(async()=>root.unmount()) }
})

it('labels a partial role reply as unfinished and keeps it outside the delivered answer', async () => {
 const el=document.createElement('div'); const root=createRoot(el)
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={{...view,purpose:'turn',unfinished:true,partial_text:'half written'}} />))
  expect(el.textContent).toContain('Unfinished partial reply')
  expect(el.textContent).toContain('half written')
 } finally { await act(async()=>root.unmount()) }
})

it('titles a role request failure and continues it without claiming a dispatch', async () => {
 const el=document.createElement('div'); const root=createRoot(el)
 const turnCard: PendingQuestion={...card,payload:{...payload,purpose:'turn',target_role:'Writer'}}
 try {
  await act(async()=>root.render(<PendingCard q={turnCard} top />))
  expect(el.textContent).toContain('Role model request failed')
  expect(el.textContent).toContain('tools that already succeeded are not run again')
  expect(el.textContent).not.toContain('Dispatch failed')
 } finally { await act(async()=>root.unmount()) }
})

it('shows retry progress and offers no second attempt while retrying', async () => {
 const retry=vi.spyOn(api,'retryModelRequest').mockResolvedValue(view)
 const retrying={...payload,state:'retrying',can_retry:false,retry_deadline_ms:Date.now()+60_000}
 const running: PendingQuestion={...card,payload:{...retrying,view:retrying}}
 useUiStore.setState({pending:[running],toasts:[],modalScope:'workbench'})
 const el=document.createElement('div'); const root=createRoot(el)
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={retrying} top />))
  expect(el.querySelector('[data-retry-progress]')?.textContent).toContain('Attempt 2')
  expect(Array.from(el.querySelectorAll('button')).some(b=>b.textContent?.includes('Retry original request'))).toBe(false)
  await approveQuestion(running)
  expect(retry).not.toHaveBeenCalled()
  expect(useUiStore.getState().toasts.at(-1)?.text).toContain('already retrying')
 } finally { await act(async()=>root.unmount()) }
})

it('routes configuration to model settings but not a temporary outage', async () => {
 const el=document.createElement('div'); const root=createRoot(el)
 const withCode=(code:string)=>({...payload,history:[{...payload.history[0],code}]})
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={withCode('configuration')} />))
  expect(el.textContent).toContain('Open model settings')
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={withCode('provider_unavailable')} />))
  expect(el.textContent).not.toContain('Open model settings')
  expect(el.textContent).toContain('Retry original request')
 } finally { await act(async()=>root.unmount()) }
})
