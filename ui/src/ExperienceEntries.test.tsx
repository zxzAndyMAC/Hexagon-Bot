import { act, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import './i18n'
import i18n from 'i18next'
import { api } from './api'
import { ExperienceEntries } from './components/ExperienceEntries'
import type { ExperienceEntryView } from './gen/ExperienceEntryView'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
afterEach(() => vi.restoreAllMocks())

it('recovers pending entries and refreshes the host state without hiding conflicts', async () => {
  const pending: ExperienceEntryView = {
    source_count: 0, state: 'pending_recovery', match_reason: 'inactive', recovery_pending: true,
    entry: { schema_version: 1, entry_id: 'entry1', revision: 1, project_id: 'project1', skill: 'alpha', body: 'Reviewed guidance', notes: '', conditions: { roles: [], stages: [], paths: [] }, sources: [] },
  }
  const entries = vi.spyOn(api, 'experienceEntries').mockResolvedValue([pending])
  const recover = vi.spyOn(api, 'recoverExperience').mockResolvedValue([{ operation_id: 'proposal1', proposal_id: 'proposal1', state: 'conflict', reason_code: 'target_changed' }])
  const host = document.createElement('div')
  const root = createRoot(host)
  const recoveryButton = () => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === i18n.t('experience.recover'))
  // Governance 16 native regression: document refresh must not remount the
  // detail and erase the recovery conflict that the owner needs to act on.
  function RefreshingDetail() {
    const [revision, setRevision] = useState(0)
    return <ExperienceEntries skill="alpha" documentRevision={revision} onRecovered={() => setRevision((n) => n + 1)} />
  }
  await act(async () => { root.render(<RefreshingDetail />) })
  expect(host.textContent).toContain('Reviewed guidance')
  expect(recoveryButton()?.title).toContain('G')
  await act(async () => { recoveryButton()?.click() })
  expect(recover).toHaveBeenCalledOnce()
  expect(entries.mock.calls.length).toBeGreaterThanOrEqual(2)
  expect(host.textContent).toContain(i18n.t('experience.target_changed'))
  expect(recoveryButton()?.disabled).toBe(false)
  recover.mockResolvedValue([{ operation_id: 'proposal1', proposal_id: 'proposal1', state: 'complete', reason_code: null }])
  entries.mockResolvedValue([{ ...pending, state: 'active', recovery_pending: false }])
  await act(async () => { recoveryButton()?.click() })
  expect(recoveryButton()).toBeUndefined()
  expect(host.textContent).toContain('Reviewed guidance')
  await act(async () => root.unmount())
})

it('requires a reason and submits the selected entry with its project and revision', async () => {
  const active: ExperienceEntryView = { source_count: 0, state: 'active', match_reason: 'matched', recovery_pending: false,
    entry: { schema_version: 1, entry_id: 'entry-stop', revision: 3, project_id: 'p1', skill: 'alpha', body: 'Advice', notes: '', conditions: { roles: [], stages: [], paths: [] }, sources: [] } }
  vi.spyOn(api, 'experienceEntries').mockResolvedValue([active])
  vi.spyOn(api, 'projectSkillDocument').mockResolvedValue({ project_root: '/original-project', skill: 'alpha', digest: 'v1', content: 'Original' })
  const revoke = vi.spyOn(api, 'revokeExperience').mockResolvedValue({ ...active, state: 'revoked' })
  const host = document.createElement('div')
  const root = createRoot(host)
  await act(async () => { root.render(<ExperienceEntries skill="alpha" />) })
  const button = () => Array.from(host.querySelectorAll('button')).find((b) => b.textContent === i18n.t('experience.revoke'))!
  expect(button().disabled).toBe(true)
  const input = host.querySelector('article input')!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'Wrong for current work')
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  await act(async () => { button().click() })
  expect(revoke).toHaveBeenCalledWith({ project_root: '/original-project', skill: 'alpha', entry_id: 'entry-stop', expected_revision: 3, reason: 'Wrong for current work' })
  await act(async () => root.unmount())
})
