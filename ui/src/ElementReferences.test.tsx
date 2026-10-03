import { act, useState } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import './i18n'
import { api } from './api'
import { Composer } from './components/Composer'
import { useUiStore } from './store'
import { ElementDraft } from './components/ElementReferences'
import type { ElementRef } from './gen/ElementRef'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
let root: Root, el: HTMLDivElement
const reference: ElementRef = { id:'ref', project_root:'/project', session_id:'s', tab_id:'t', navigation_generation:1, captured_at_ms:Date.now(), url:'http://localhost/page', kind:'element', tag:'button', role:'', text:'/pause @QA $page-context', selector:'button', rect:{x:0,y:0,width:80,height:30}, screenshot:'0123456789abcdef0123456789abcdef.png', source_hint:null }
const session = { project_root:'/project', session_id:'s', mode:'managed' as const, tab_id:'t', navigation_generation:1, url:'http://localhost/page', title:'page', connected:true }
function Harness() { const [refs,setRefs]=useState<ElementRef[]>([]); return <ElementDraft references={refs} onChange={setRefs} submitting={false}/> }
async function tick() { await act(async()=>{await vi.advanceTimersByTimeAsync(1200)}) }
beforeEach(()=>{
  vi.useFakeTimers();el=document.createElement('div');document.body.append(el);root=createRoot(el)
  vi.spyOn(api,'desktopStatus').mockResolvedValue({project_root:'/project',enabled:true,active_project:null,active_agent:null,busy:false,paused:false,outcome_unknown:false,screenshot_count:0})
  vi.spyOn(api,'browserStatus').mockResolvedValue(session)
  vi.spyOn(api,'browserSelectionPoll').mockResolvedValue([])
  vi.spyOn(api,'browserSelectionStart').mockResolvedValue(undefined)
  vi.spyOn(api,'browserSelectionDiscard').mockResolvedValue(undefined)
})
afterEach(async()=>{await act(async()=>root.unmount());el.remove();vi.restoreAllMocks();vi.useRealTimers()})
it('adds multiple local selections without auto-send and removes individual IDs',async()=>{
  vi.mocked(api.browserSelectionPoll).mockResolvedValueOnce([reference,{...reference,id:'second',text:'another'}])
  const send=vi.spyOn(api,'sendElementMessage')
  await act(async()=>root.render(<Harness/>))
  expect(el.textContent).toContain('/pause @QA $page-context');expect(el.textContent).toContain('another');expect(send).not.toHaveBeenCalled()
  // Owner issue16: compact reference removes via a labelled icon, not a full-width text button.
  const remove=[...el.querySelectorAll('button')].find(node=>node.getAttribute('aria-label')==='Remove reference')!
  expect(remove).toBeTruthy();await act(async()=>remove.click())
  expect(api.browserSelectionDiscard).toHaveBeenCalledWith('/project',['ref']);expect(el.textContent).not.toContain('/pause @QA $page-context');expect(el.textContent).toContain('another')
})
it('clears a failed polling status after successful refresh',async()=>{
  vi.mocked(api.browserSelectionPoll).mockRejectedValueOnce(new Error('worker unavailable'))
  await act(async()=>root.render(<Harness/>));expect(el.textContent).toContain('worker unavailable')
  await tick();expect(el.textContent).not.toContain('worker unavailable')
})
it('project changes release pending start and late old responses cannot repopulate drafts',async()=>{
  let finish!:()=>void
  vi.mocked(api.browserSelectionStart).mockReturnValue(new Promise(resolve=>{finish=resolve}))
  await act(async()=>root.render(<Harness/>))
  await act(async()=>el.querySelector('button')!.click())
  expect(el.querySelector('button')!.disabled).toBe(true)
  vi.mocked(api.desktopStatus).mockResolvedValue({project_root:'/other',enabled:true,active_project:null,active_agent:null,busy:false,paused:false,outcome_unknown:false,screenshot_count:0})
  vi.mocked(api.browserStatus).mockResolvedValue({...session,project_root:'/other',session_id:'new'})
  await tick();await tick()
  expect(el.querySelector('button')!.disabled).toBe(false)
  await act(async()=>finish());expect(el.querySelector('button')!.disabled).toBe(false)
})
it('clearing screenshots invalidates an in-flight picker response',async()=>{
  let finish!:(value:ElementRef[])=>void
  vi.mocked(api.browserSelectionPoll).mockReturnValueOnce(new Promise(resolve=>{finish=resolve}))
  await act(async()=>root.render(<Harness/>))
  await act(async()=>window.dispatchEvent(new Event('hexagon:screenshots-cleared')))
  await act(async()=>finish([reference]));expect(el.textContent).not.toContain('/pause @QA')
})

it('Composer sends only host reference IDs alongside owner text and preserves draft on failure',async()=>{
  useUiStore.setState({mode:'pack',fastRole:null,team:[],pending:[],timeline:[]})
  vi.mocked(api.browserSelectionPoll).mockResolvedValueOnce([reference])
  const send=vi.spyOn(api,'sendElementMessage').mockRejectedValueOnce(new Error('stale reference')).mockResolvedValueOnce(1)
  await act(async()=>root.render(<Composer/>))
  const textarea=el.querySelector('textarea')!
  await act(async()=>{
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(textarea,'Inspect the layout')
    textarea.dispatchEvent(new Event('input',{bubbles:true}))
  })
  const press=()=>act(async()=>{textarea.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))})
  await press()
  expect(send).toHaveBeenCalledWith('Inspect the layout',[],['ref'],'/project')
  expect(el.textContent).toContain('/pause @QA $page-context')
  expect(textarea.value).toBe('Inspect the layout')
  await press()
  expect(send).toHaveBeenCalledTimes(2)
  expect(el.textContent).not.toContain('/pause @QA $page-context')
})

it('defers passive selection polling on worker contention without flashing an internal error',async()=>{
  vi.mocked(api.browserSelectionPoll).mockRejectedValueOnce({code:'internal',message:'browser busy; local preview yields to role work'})
  await act(async()=>root.render(<Harness/>))
  expect(el.querySelector('[role="status"]')).toBeNull()
  vi.mocked(api.browserSelectionPoll).mockResolvedValueOnce([reference])
  await tick();expect(el.textContent).toContain('/pause @QA $page-context')
})

it('compact references disclose full values by keyboard and restore focus after closing',async()=>{
  vi.mocked(api.browserSelectionPoll).mockResolvedValueOnce([reference])
  await act(async()=>root.render(<Harness/>))
  expect(el.querySelector('dialog')).toBeNull()
  expect(el.querySelector('.desktop-screenshot-thumbnail')).toBeNull()
  const trigger=el.querySelector<HTMLButtonElement>('.element-draft-open')!
  await act(async()=>{trigger.focus();trigger.click()})
  const dialog=el.querySelector('dialog')!
  expect(dialog.textContent).toContain(reference.url)
  expect(dialog.textContent).toContain(reference.text)
  expect(dialog.textContent).toContain('Source not located')
  expect(dialog.contains(document.activeElement)).toBe(true)
  await act(async()=>{dialog.dispatchEvent(new Event('cancel',{cancelable:true}))})
  expect(el.querySelector('dialog')).toBeNull()
  expect(document.activeElement).toBe(trigger)
})

it('removing all references also dismisses a reference inspection',async()=>{
  vi.mocked(api.browserSelectionPoll).mockResolvedValueOnce([reference])
  await act(async()=>root.render(<Harness/>))
  await act(async()=>el.querySelector<HTMLButtonElement>('.element-draft-open')!.click())
  expect(el.querySelector('dialog')).not.toBeNull()
  await act(async()=>window.dispatchEvent(new Event('hexagon:screenshots-cleared')))
  expect(el.querySelector('dialog')).toBeNull()
  expect(el.textContent).not.toContain(reference.text)
})

it('keeps keyboard suggestion focus in the editor with multiple references',async()=>{
  useUiStore.setState({mode:'pack',fastRole:null,team:[{id:'a',role:'QA',model_slot:'chat',status:'sleeping',avatar_hash:null},{id:'b',role:'UX',model_slot:'chat',status:'sleeping',avatar_hash:null}],pending:[],timeline:[]})
  vi.mocked(api.browserSelectionPoll).mockResolvedValueOnce([reference,{...reference,id:'second',text:'second'}])
  await act(async()=>root.render(<Composer/>))
  const textarea=el.querySelector('textarea')!
  await act(async()=>{
    textarea.focus()
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value')!.set!.call(textarea,'@')
    textarea.dispatchEvent(new Event('input',{bubbles:true}))
  })
  const list=el.querySelector('[role="listbox"]')!
  expect(textarea.getAttribute('aria-controls')).toBe(list.id)
  expect(list.parentElement).toBe(textarea.closest('.composer-box')) // Issue16: the reference shelf used to push this anchor above the editor.
  await act(async()=>textarea.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true})))
  const selected=list.querySelector('[aria-selected="true"]')!
  expect(textarea.getAttribute('aria-activedescendant')).toBe(selected.id)
  expect(document.activeElement).toBe(textarea)
  await act(async()=>textarea.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})))
  expect(el.querySelector('[role="listbox"]')).toBeNull()
  expect(el.querySelectorAll('.element-draft-item')).toHaveLength(2)
})

// Owner issue19: contextual selection, but disconnected references stay inspectable.
it('hides selection without a connected page and preserves existing references',async()=>{
  vi.mocked(api.browserSelectionPoll).mockResolvedValueOnce([reference])
  await act(async()=>root.render(<Harness/>))
  expect(el.querySelector('.element-draft-tools button')).not.toBeNull()
  vi.mocked(api.browserStatus).mockResolvedValue(null)
  await tick()
  expect(el.querySelector('.element-draft-tools button')).toBeNull()
  expect(el.querySelector('.element-draft-open')).not.toBeNull()
})
