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
  // 功能缺口 D3：行改为原型的单行 srow（标题 + 来源 + chip），停用理由等治理细节收进展开体——先展开该行。
  expect(host.querySelector('article input')).toBeNull()
  await act(async () => { host.querySelector<HTMLButtonElement>('article button[aria-expanded]')!.click() })
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

it('renders the prototype header, one-line rows and history for the open entry (gap D3)', async () => {
  const source = { review_event: 7, activation: 1, artifact_id: 'r7', author: 'Lead review', artifact_version: 3, artifact_digest: 'd' }
  const make = (entry_id: string, body: string, state: ExperienceEntryView['state'], sources = [source], source_count = 1): ExperienceEntryView => ({
    source_count, state, match_reason: 'matched', recovery_pending: false,
    entry: { schema_version: 1, entry_id, revision: 3, project_id: 'p', skill: 'alpha', body, notes: '', conditions: { roles: [], stages: [], paths: [] }, sources } })
  vi.spyOn(api, 'experienceEntries').mockResolvedValue([make('e1', '## Safari throws on setItem\nmore', 'active'), make('e2', 'Hash cache names', 'changed', [], 0)])
  vi.spyOn(api, 'projectSkillDocument').mockResolvedValue({ project_root: '/p', skill: 'alpha', digest: 'v1', content: 'x' })
  const history = vi.spyOn(api, 'experienceHistory').mockResolvedValue({ items: [], next_cursor: null })
  await i18n.changeLanguage('en')
  const host = document.createElement('div')
  const root = createRoot(host)
  await act(async () => { root.render(<ExperienceEntries skill="alpha" />) })
  expect(host.textContent).toContain(i18n.t('experience.entriesN', { count: 2 }))
  const rows = [...host.querySelectorAll('article')]
  expect(rows[0].querySelector('b')?.textContent).toBe('Safari throws on setItem')
  expect(rows[0].textContent).toContain('Source: Lead review · v3')
  expect(rows[1].textContent).toContain('No source recorded · v3')
  expect(rows[1].textContent).toContain(i18n.t('experience.changed'))
  // 无原生 details 三角：整理与限额是 aria-expanded 折叠行
  expect(host.querySelector('details, fieldset')).toBeNull()
  expect([...host.querySelectorAll('[data-experience-disclosure] button')].map((b) => b.getAttribute('aria-expanded'))).toEqual(['false', 'false'])
  // 「历史」看展开的那条
  await act(async () => { rows[1].querySelector<HTMLButtonElement>('button[aria-expanded]')!.click() })
  const historyBtn = [...host.querySelectorAll('button')].find((b) => b.textContent === i18n.t('experience.historyShort'))!
  await act(async () => { historyBtn.click() })
  expect(historyBtn.getAttribute('aria-pressed')).toBe('true')
  expect(history).toHaveBeenLastCalledWith(expect.objectContaining({ entry_id: 'e2' }))
  expect(host.querySelector('[data-experience-history]')?.textContent).toContain('History · Hash cache names')
  await act(async () => root.unmount())
})

it('explains the empty list instead of rendering nothing (gap D3)', async () => {
  vi.spyOn(api, 'experienceEntries').mockResolvedValue([])
  vi.spyOn(api, 'projectSkillDocument').mockResolvedValue({ project_root: '/p', skill: 'alpha', digest: 'v1', content: 'x' })
  const host = document.createElement('div')
  const root = createRoot(host)
  await act(async () => { root.render(<ExperienceEntries skill="alpha" />) })
  expect(host.textContent).toContain(i18n.t('experience.emptyEntries'))
  const historyBtn = [...host.querySelectorAll('button')].find((b) => b.textContent === i18n.t('experience.historyShort'))!
  expect(historyBtn.getAttribute('aria-disabled')).toBe('true')
  expect(historyBtn.title).toBe(i18n.t('experience.historyNone'))
  await act(async () => root.unmount())
})
