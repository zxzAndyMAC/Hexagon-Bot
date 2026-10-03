import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { ProjectApprovalMode } from './components/ProjectApprovalMode'
import { api } from './api'
import { useUiStore } from './store'
import i18n from './i18n'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(async () => { await act(async () => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })
it('never inherits the previous workspace mode and ignores a late read after unmount', async () => {
  await i18n.changeLanguage('en')
  let resolve!: (v: { project_id: string; project_root: string; mode: 'broad' }) => void
  const read = vi.spyOn(api, 'approvalMode').mockImplementationOnce(() => new Promise(r => { resolve = r })).mockResolvedValue({ project_id: 'new', project_root: '/new', mode: 'restricted' })
  document.body.appendChild(el); root = createRoot(el)
  await act(async () => root.render(<ProjectApprovalMode />))
  expect(el.textContent).toContain('Loading approval mode')
  await act(async () => { root.unmount(); root = createRoot(el); root.render(<ProjectApprovalMode />) })
  await act(async () => resolve({ project_id: 'old', project_root: '/old', mode: 'broad' }))
  expect(read).toHaveBeenCalledTimes(2)
  expect(el.textContent).toContain('Restricted access')
  expect(el.textContent).not.toContain('Broad access')
})
it('keeps the persisted mode after a save failure and allows retry', async () => {
  vi.spyOn(api, 'approvalMode').mockResolvedValue({ project_id: 'p', project_root: '/project-a', mode: 'restricted' })
  const save = vi.spyOn(api, 'setApprovalMode').mockRejectedValueOnce(new Error('disk')).mockResolvedValue({ project_id: 'p', project_root: '/project-a', mode: 'assisted' })
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<ProjectApprovalMode />))
  await act(async () => el.querySelector('button')!.click())
  await act(async () => el.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')[1].click())
  expect(el.querySelector('[role=alert]')).not.toBeNull()
  expect(el.querySelector('[aria-checked=true]')?.textContent).toContain('Restricted access')
  await act(async () => el.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')[1].click())
  expect(save).toHaveBeenCalledTimes(2)
  expect(save).toHaveBeenLastCalledWith('assisted', '/project-a')
  expect(el.querySelector('button')?.textContent).toContain('Help me approve')
})
it('binds consent to its project and drops late saves after changing workspace', async () => {
  vi.spyOn(api, 'approvalMode')
    .mockResolvedValueOnce({ project_id: 'a', project_root: '/a', mode: 'restricted' })
    .mockResolvedValueOnce({ project_id: 'b', project_root: '/b', mode: 'restricted' })
  let resolve!: (v: { project_id: string; project_root: string; mode: 'broad' }) => void
  const save = vi.spyOn(api, 'setApprovalMode').mockImplementation(() => new Promise(r => { resolve = r }))
  const refresh = vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<ProjectApprovalMode key="a" />))
  await act(async () => el.querySelector('button')!.click())
  await act(async () => el.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')[2].click())
  expect(save).not.toHaveBeenCalled()
  await act(async () => el.querySelectorAll<HTMLButtonElement>('[role=alertdialog] button')[1].click())
  expect(save).toHaveBeenCalledExactlyOnceWith('broad', '/a')
  await act(async () => root.render(<ProjectApprovalMode key="b" />))
  expect(el.querySelector('[role=alertdialog]')).toBeNull()
  await act(async () => resolve({ project_id: 'a', project_root: '/a', mode: 'broad' }))
  expect(el.querySelector('button')?.textContent).toContain('Restricted access')
  expect(refresh).not.toHaveBeenCalled()
})
it('discards an unconfirmed dialog when the initiating workspace closes', async () => {
  vi.spyOn(api, 'approvalMode').mockResolvedValue({ project_id: 'a', project_root: '/a', mode: 'restricted' })
  const save = vi.spyOn(api, 'setApprovalMode')
  root = createRoot(el); document.body.appendChild(el)
  await act(async () => root.render(<ProjectApprovalMode />))
  await act(async () => el.querySelector('button')!.click())
  await act(async () => el.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')[2].click())
  await act(async () => root.render(null))
  expect(el.querySelector('[role=alertdialog]')).toBeNull()
  expect(save).not.toHaveBeenCalled()
})
