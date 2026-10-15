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
import { isMac } from './keymap'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({data,itemContent}: {data: unknown[];itemContent: (i:number,row:unknown)=>ReactNode}) => <div>{data.map((r,i)=><div key={i}>{itemContent(i,r)}</div>)}</div>,
}))
;(globalThis as Record<string,unknown>).IS_REACT_ACT_ENVIRONMENT=true
const view: ModelRequestRecovery = {recovery_id:'q1',purpose:'pm_route',state:'failed',project_root:'/project',agent_id:'pm',stage_run_id:null,target_role:'Product',history:[{code:'transport',model:null,model_slot:'decision',request_id:'request1',http_status:null,message:'unexpected EOF'}],can_retry:true,partial_text:null,unfinished:false,retry_deadline_ms:null,instruction:null,last_refusal:null,superseded_reason:null,held_instruction:null,chat_model_slot:null}
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
  // 票 08（components D11 ③）：钥匙类失败的入口改叫「更新钥匙」，在钥匙行里；仍去模型设置。
  expect(el.querySelector('[data-remedy="missing_credential"]')!.textContent).toContain('Update key')
  expect(el.textContent).not.toContain('Usage and quota')
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

it('asks the owner to choose and never lets the shortcut pick for them', async () => {
 const resolve=vi.spyOn(api,'resolveModelRequestChoice').mockResolvedValue(view)
 const retry=vi.spyOn(api,'retryModelRequest').mockResolvedValue(view)
 const choosing={...payload,purpose:'turn',state:'needs_choice',can_retry:false,held_instruction:'write the changelog instead'}
 const held: PendingQuestion={...card,payload:{...choosing,view:choosing}}
 useUiStore.setState({pending:[held],toasts:[],modalScope:'workbench'})
 const el=document.createElement('div'); const root=createRoot(el)
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={choosing} top />))
  expect(el.textContent).toContain('write the changelog instead')
  await approveQuestion(held)
  expect(retry).not.toHaveBeenCalled(); expect(resolve).not.toHaveBeenCalled()
  expect(useUiStore.getState().toasts.at(-1)?.text).toContain('Choose to continue the original task')
  const follow=Array.from(el.querySelectorAll('button')).find(b=>b.textContent==='Follow new instruction')!
  expect(follow.title).toContain('·')
  await act(async()=>follow.click())
  expect(resolve).toHaveBeenCalledWith('q1',true,'/project')
  await act(async()=>{ window.dispatchEvent(new KeyboardEvent('keydown',{key:'c',code:'KeyC',altKey:true,metaKey:isMac,ctrlKey:!isMac})) })
  expect(resolve).toHaveBeenLastCalledWith('q1',false,'/project')
 } finally { await act(async()=>root.unmount()) }
})

it('shows why a retired entry can no longer send and offers no action', async () => {
 const el=document.createElement('div'); const root=createRoot(el)
 const retired={...payload,purpose:'turn',state:'superseded',can_retry:false,superseded_reason:'stage_changed'}
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={retired} top />))
  expect(el.textContent).toContain('No longer valid')
  expect(el.textContent).toContain('a new stage started')
  expect(el.querySelectorAll('button')).toHaveLength(0)
 } finally { await act(async()=>root.unmount()) }
})

it('marks an attempt interrupted by exit as unknown and keeps the original input', async () => {
 const el=document.createElement('div'); const root=createRoot(el)
 const unknown={...payload,purpose:'turn',state:'unknown',instruction:'Draft the spec.',last_refusal:'checkpoint_missing'}
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={unknown} />))
  expect(el.textContent).toContain('Result unknown')
  expect(el.textContent).toContain('Original instruction: Draft the spec.')
  expect(el.querySelector('[data-refusal]')?.textContent).toContain('no exact material')
  expect(Array.from(el.querySelectorAll('button')).some(b=>b.textContent?.includes('Retry original request'))).toBe(true)
 } finally { await act(async()=>root.unmount()) }
})

it('offers the chat model only as an explicit action the approve shortcut never takes', async () => {
 const retry=vi.spyOn(api,'retryModelRequest').mockResolvedValue(view)
 const chat=vi.spyOn(api,'retryModelRequestWithChatModel').mockResolvedValue(view)
 const offer={...payload,chat_model_slot:'default'}
 const failed: PendingQuestion={...card,payload:{...offer,view:offer}}
 useUiStore.setState({pending:[failed],toasts:[],modalScope:'workbench'})
 const el=document.createElement('div'); const root=createRoot(el)
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={offer} top />))
  const button=el.querySelector('button[data-chat-model]') as HTMLButtonElement
  expect(button.textContent).toContain('Retry with chat model (default)')
  expect(button.title).toContain('No model settings change')
  await approveQuestion(failed)
  expect(retry).toHaveBeenCalledWith('q1','/project'); expect(chat).not.toHaveBeenCalled()
  await act(async()=>{ window.dispatchEvent(new KeyboardEvent('keydown',{key:'m',code:'KeyM',altKey:true,metaKey:isMac,ctrlKey:!isMac})) })
  expect(chat).toHaveBeenCalledWith('q1','/project')
 } finally { await act(async()=>root.unmount()) }
})

it('re-runs a failed opening analysis through its own lock-free command', async () => {
 const intake=vi.spyOn(api,'runOpeningIntake').mockResolvedValue(undefined)
 const retry=vi.spyOn(api,'retryModelRequest').mockResolvedValue(view)
 const failed={...payload,purpose:'opening_intake',target_role:null}
 const item: PendingQuestion={...card,payload:{...failed,view:failed}}
 useUiStore.setState({pending:[item],toasts:[],modalScope:'workbench'})
 const el=document.createElement('div'); const root=createRoot(el)
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={failed} top />))
  expect(el.textContent).toContain('Opening analysis failed')
  await approveQuestion(item)
  expect(intake).toHaveBeenCalledTimes(1); expect(retry).not.toHaveBeenCalled()
 } finally { await act(async()=>root.unmount()) }
})
it('stops automatic retries even while a retry is in flight, with its own binding', async () => {
 const stop=vi.spyOn(api,'stopModelRequest').mockResolvedValue(null)
 const retrying={...payload,state:'retrying',can_retry:false,retry_deadline_ms:Date.now()+60_000}
 useUiStore.setState({pending:[],team:[],toasts:[],modalScope:'workbench'})
 const el=document.createElement('div');const root=createRoot(el)
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q1" payload={retrying} top />))
  const button=el.querySelector<HTMLButtonElement>('[data-stop-retry]')!
  expect(button.textContent).toBe('Stop retrying')
  expect(button.disabled).toBe(false)
  await act(async()=>button.click())
  expect(stop).toHaveBeenCalledWith('q1')
  // Ticket 09 / alignment: the chat-model retry never shares the approve key.
  const keys=await import('./keymap')
  expect(keys.bindingFor('retryWithChatModel')).not.toBe(keys.bindingFor('approve'))
 }finally {await act(async()=>root.unmount())}
})
it('a role turn item waiting for the network has the same stop entry and binding', async () => {
 // Managed-followups ticket 03: the per-item stop is no longer PM-only.
 const stop=vi.spyOn(api,'stopModelRequest').mockResolvedValue(null)
 const keys=await import('./keymap')
 const turn={...payload,purpose:'turn',agent_id:'a0',target_role:'Designer',state:'retrying',can_retry:false,retry_deadline_ms:Date.now()+60_000,history:[]}
 useUiStore.setState({pending:[],team:[],toasts:[],modalScope:'workbench'})
 const el=document.createElement('div');document.body.appendChild(el);const root=createRoot(el)
 try {
  await act(async()=>root.render(<ModelRecoveryStatus id="q2" payload={turn} top />))
  const button=el.querySelector<HTMLButtonElement>('[data-stop-retry]')!
  expect(button.title).toContain(keys.formatBinding(keys.bindingFor('modelRecoveryStop')))
  await act(async()=>button.click())
  expect(stop).toHaveBeenCalledWith('q2')
 }finally {await act(async()=>root.unmount()); el.remove()}
})
