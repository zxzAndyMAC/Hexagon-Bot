import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import './i18n'
import { api, type PendingQuestion } from './api'
import { PendingCard } from './components/PendingCards'
import { approveQuestion } from './decisions'
import { useUiStore } from './store'
import { isMac } from './keymap'

;(globalThis as Record<string,unknown>).IS_REACT_ACT_ENVIRONMENT=true
afterEach(()=>vi.restoreAllMocks())

const permission: PendingQuestion = {id:'q7',kind:'permission',agent_id:'a0',state:'queued',payload:{
  tool:'bash',input:{cmd:'cargo test'},safety_net:false,reviewer_unsure:'model_request_failed',
  model_failure:{purpose:'action_review',code:'transport',model_slot:'default',request_id:'request9'}}}

it('labels a judge request failure as a failure, not a verdict, and re-judges only on its own action', async () => {
  const rejudge=vi.spyOn(api,'rejudgeAfterModelFailure').mockResolvedValue('held')
  const allow=vi.spyOn(api,'answerPermission').mockResolvedValue(undefined as never)
  useUiStore.setState({pending:[permission],toasts:[],modalScope:'workbench',projectRoot:'/project'})
  const el=document.createElement('div'); const root=createRoot(el)
  try {
    await act(async()=>root.render(<PendingCard q={permission} top />))
    expect(el.textContent).toContain('Action review model request failed')
    expect(el.textContent).toContain('not a verdict')
    const button=Array.from(el.querySelectorAll('button')).find(b=>b.textContent==='Re-judge')!
    expect(button.title).toContain('·')
    await approveQuestion(permission)
    expect(allow).toHaveBeenCalledWith('q7',true)
    expect(rejudge).not.toHaveBeenCalled()
    await act(async()=>button.click())
    expect(rejudge).toHaveBeenCalledWith('q7','/project')
    expect(useUiStore.getState().toasts.at(-1)?.text).toContain('still leaves this decision to you')
    await act(async()=>{ window.dispatchEvent(new KeyboardEvent('keydown',{key:'j',code:'KeyJ',altKey:true,metaKey:isMac,ctrlKey:!isMac})) })
    expect(rejudge).toHaveBeenCalledTimes(2)
  } finally { await act(async()=>root.unmount()) }
})

it('shows an advice-model failure as missing advice, not a recommendation', async () => {
  const stamp: PendingQuestion={id:'q8',kind:'stamp',agent_id:'a0',state:'queued',payload:{proposal_id:'p1',surface:'agents_md',
    judge_verdict:'needs_human',judge_reason:{code:'model_request_failed',failure:'provider_unavailable'}}}
  const el=document.createElement('div'); const root=createRoot(el)
  try {
    await act(async()=>root.render(<PendingCard q={stamp} top={false} />))
    expect(el.querySelector('[data-judge-failure="advice"]')?.textContent).toContain('provider_unavailable')
    expect(el.textContent).not.toContain('Re-judge')
  } finally { await act(async()=>root.unmount()) }
})
