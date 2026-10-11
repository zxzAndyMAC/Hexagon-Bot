import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { ProjectApprovalMode } from './components/ProjectApprovalMode'
import { ManagedPill } from './components/ManagedPill'
import { ProjectDelegation } from './components/ProjectDelegation'
import { TopBar } from './components/TopBar'
import { api } from './api'
import { useUiStore } from './store'
import i18n from './i18n'
import type { DelegationStatus } from './gen/DelegationStatus'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
const off: DelegationStatus = { enabled: false, paused: false, generation: 0, delegation_id: null }
const on: DelegationStatus = { enabled: true, paused: false, generation: 1, delegation_id: 'g1' }

beforeEach(async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'approvalMode').mockResolvedValue({ project_id: 'p1', project_root: '/proj', mode: 'assisted' })
  useUiStore.setState({ projectRoot: '/proj' })
  vi.spyOn(api, 'managedHistory').mockResolvedValue([])
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([])
})
afterEach(async () => { await act(async () => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })

async function mount(node: React.ReactNode) {
  document.body.appendChild(el); root = createRoot(el)
  await act(async () => root.render(node))
}

it('delegation is a separate switch in the approval menu, explained once, never optimistic', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(off)
  let resolve!: (value: DelegationStatus) => void
  const save = vi.spyOn(api, 'setProjectDelegation').mockImplementation(() => new Promise(r => { resolve = r }))
  await mount(<ProjectApprovalMode />)
  const trigger = el.querySelector<HTMLButtonElement>('.approval-mode-trigger')!
  expect(trigger.textContent).toBe('Help me approve')
  await act(async () => trigger.click())
  expect(el.querySelectorAll('[role="menuitemradio"]')).toHaveLength(3)
  const toggle = el.querySelector<HTMLInputElement>('input[role="switch"]')!
  expect(toggle.checked).toBe(false)
  await act(async () => toggle.click())
  expect(save).not.toHaveBeenCalled()
  const dialog = el.querySelector('.managed-confirm')!
  expect(dialog.textContent).toContain('The system will not')
  expect(dialog.textContent).toContain('at most 3 correction rounds')
  expect(document.activeElement?.textContent).toBe('Cancel')
  const confirm = [...dialog.querySelectorAll('button')].find(b => b.textContent === 'Turn on delegation')!
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  await act(async () => confirm.click())
  expect(save).toHaveBeenCalledWith(true, '/proj')
  expect(trigger.textContent).toBe('Help me approve', /* host has not answered yet */)
  await act(async () => resolve(on))
  expect(trigger.textContent).toBe('Help me approve | Delegation')
})

it('the resident pill is neutral, pauses and turns off through the host', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([{ id: 'q1', kind: 'permission', agent_id: 'a0', payload: {}, state: 'queued' }])
  const pause = vi.spyOn(api, 'setProjectDelegationPaused').mockResolvedValue({ ...on, paused: true, generation: 2 })
  const disable = vi.spyOn(api, 'setProjectDelegation').mockResolvedValue(off)
  await mount(<ManagedPill />)
  const pill = el.querySelector<HTMLButtonElement>('.managed-pill')!
  expect(pill.textContent).toContain('Delegating')
  expect(pill.className).not.toContain('amber')
  await act(async () => pill.click())
  expect(el.textContent).toContain('Processing 1 pending')
  const pauseButton = [...el.querySelectorAll('button')].find(b => b.textContent === 'Pause delegation')!
  vi.spyOn(api, 'projectDelegation').mockResolvedValue({ ...on, paused: true, generation: 2 })
  await act(async () => pauseButton.click())
  expect(pause).toHaveBeenCalledWith(true, '/proj')
  expect(el.querySelector('[data-managed-state]')!.getAttribute('data-managed-state')).toBe('paused')
  const offButton = [...el.querySelectorAll('button')].find(b => b.textContent === 'Turn off project delegation')!
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(off)
  await act(async () => offButton.click())
  expect(disable).toHaveBeenCalledWith(false, '/proj')
  expect(el.querySelector('.managed-pill')).toBeNull()
})

it('the pending badge counts only owner items; delegated ones show in the pill', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([{ id: 'q9', kind: 'permission', agent_id: 'a0', payload: {}, state: 'queued' }])
  vi.spyOn(api, 'sandboxStatus').mockResolvedValue({ available: false, mode: 'none', note: '' })
  // The host's pending read already excludes delegated cards (owner_pending).
  useUiStore.setState({ projectName: 'p', packName: 'x', mode: 'pack', pending: [], reviewRows: [], team: [], railOpen: false, projectRoot: '/proj' })
  await mount(<TopBar onSettings={() => {}} onProjectClosed={() => {}} />)
  expect(el.querySelector('[data-pending-count]')).toBeNull()
  expect(el.querySelector('.managed-pill')).not.toBeNull()
})

it('settings shows status and the report, not a second switch', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedHistory').mockResolvedValue([
    { event_id: 9, card_id: 'q7', card_kind: 'permission', action: 'deny', reason: 'budget_exhausted', via: 'host', outcome: 'denied', actor: 'managed:g:3', result: 'done' },
  ])
  await mount(<ProjectDelegation />)
  expect(el.querySelector('input[role="switch"]')).toBeNull()
  expect(el.textContent).toContain('approval mode menu')
  expect(el.textContent).toContain('Refused')
  expect(el.textContent).toContain('1 done')
})
