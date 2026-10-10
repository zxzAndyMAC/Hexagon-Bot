import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { ProjectDelegation } from './components/ProjectDelegation'
import { api } from './api'
import i18n from './i18n'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(async () => { await act(async () => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })

async function mount() {
  document.body.appendChild(el); root = createRoot(el)
  await act(async () => root.render(<ProjectDelegation />))
}

it('turning delegation on needs an explicit confirmation and the host-persisted grant', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'approvalMode').mockResolvedValue({ project_id: 'p1', project_root: '/proj', mode: 'restricted' })
  vi.spyOn(api, 'projectDelegation').mockResolvedValue({ enabled: false, generation: 0, delegation_id: null })
  vi.spyOn(api, 'managedHistory').mockResolvedValue([])
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([])
  const save = vi.spyOn(api, 'setProjectDelegation').mockResolvedValue({ enabled: true, generation: 1, delegation_id: 'g1' })
  await mount()
  const toggle = el.querySelector<HTMLButtonElement>('button[aria-pressed]')!
  expect(toggle.textContent).toBe('Delegation off')
  expect(toggle.title).toContain('Turn on project delegation')
  expect(toggle.title).toMatch(/G/)
  await act(async () => toggle.click())
  expect(save).not.toHaveBeenCalled()
  const confirm = [...el.querySelectorAll('button')].find(b => b.textContent === 'Turn on delegation')!
  await act(async () => confirm.click())
  expect(save).toHaveBeenCalledWith(true, '/proj')
  expect(el.querySelector('button[aria-pressed]')!.textContent).toBe('Delegation on')
})

it('a failed save keeps the previous state and shows managed history', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'approvalMode').mockResolvedValue({ project_id: 'p1', project_root: '/proj', mode: 'restricted' })
  vi.spyOn(api, 'projectDelegation').mockResolvedValue({ enabled: true, generation: 3, delegation_id: 'g' })
  vi.spyOn(api, 'managedHistory').mockResolvedValue([
    { event_id: 9, card_id: 'q7', card_kind: 'permission', action: 'deny', reason: 'budget_exhausted', via: 'host', outcome: 'denied', actor: 'managed:g:3' },
  ])
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([{ id: 'q8', kind: 'permission', agent_id: 'a0', payload: {}, state: 'queued' }])
  vi.spyOn(api, 'setProjectDelegation').mockRejectedValue(new Error('mismatch'))
  await mount()
  expect(el.textContent).toContain('Processing 1 pending')
  expect(el.textContent).toContain('Refused')
  expect(el.textContent).toContain('host rule')
  await act(async () => el.querySelector<HTMLButtonElement>('button[aria-pressed]')!.click())
  expect(el.textContent).toContain('Could not save')
  expect(el.querySelector('button[aria-pressed]')!.textContent).toBe('Delegation on')
})
