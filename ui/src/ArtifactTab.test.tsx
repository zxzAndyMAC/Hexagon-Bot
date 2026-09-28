import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api } from './api'
import { useUiStore } from './store'
import { ArtifactTab } from './components/ArtifactTab'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it('shows current worktree by default and explicit older snapshots separately', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ artifacts: [1, 2].map((version) => ({
    id: `art${version}`, path: 'notes.txt', kind: 'doc', tier: 'freeform',
    stage_run_id: null, author: null, version, status: version === 1 ? 'superseded' : 'valid', upstream_id: null,
  })) })
  vi.spyOn(api, 'proposals').mockResolvedValue([])
  vi.spyOn(api, 'artifactContent').mockResolvedValue('Owner worktree edit')
  vi.spyOn(api, 'artifactContentAt').mockImplementation(async (_path, version) => version === 1 ? 'Earlier snapshot' : 'Registered snapshot')
  const el = document.createElement('div')
  document.body.append(el)
  const root = createRoot(el)
  await act(async () => root.render(<ArtifactTab path="notes.txt" />))
  expect(el.textContent).toContain('Owner worktree edit')
  expect(el.textContent).toContain('Current worktree')
  const older = [...el.querySelectorAll('button')].find((button) => button.textContent === 'v1')
  expect(older).toBeTruthy()
  await act(async () => older?.click())
  expect(el.textContent).toContain('Earlier snapshot')
  expect(el.textContent).not.toContain('Owner worktree edit')
  await act(async () => root.unmount())
  el.remove()
  useUiStore.setState(saved, true)
  vi.restoreAllMocks()
})

it('shows stale review evidence with the reviewed version and reviewer', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  useUiStore.setState({ artifacts: [{
    id: 'art2', path: 'notes.txt', kind: 'doc', tier: 'freeform', stage_run_id: 'r1',
    author: 'a0', version: 2, status: 'valid', upstream_id: null,
    review: { status: 'stale', event_id: 7, reviewer: 'Reviewer', version: 1, digest: 'old-digest' },
  }] })
  vi.spyOn(api, 'proposals').mockResolvedValue([])
  vi.spyOn(api, 'artifactContent').mockResolvedValue('Current document')
  const el = document.createElement('div')
  const root = createRoot(el)
  await act(async () => root.render(<ArtifactTab path="notes.txt" />))
  expect(el.textContent).toContain('Review outdated')
  expect(el.textContent).toContain('Reviewer · v1')
  await act(async () => root.unmount())
  useUiStore.setState(saved, true)
  vi.restoreAllMocks()
})

it('withdraws the selected governed contribution only after the owner confirms', async () => {
  await i18n.changeLanguage('en')
  const saved = useUiStore.getState()
  const askConfirm = vi.fn<typeof saved.askConfirm>()
  useUiStore.setState({ artifacts: [], askConfirm })
  vi.spyOn(api, 'proposals').mockResolvedValue([{ id: 'experience-prop', surface: 'skill', target: 'alpha', status: 'active', author: 'a0', artifact_path: 'props/lesson.md' }])
  vi.spyOn(api, 'experienceProposal').mockResolvedValue({ proposal_id: 'experience-prop', previous_entries: [], legacy: null, recovery_pending: false,
    request: { create_role_skill: false, body: 'Reviewed lesson', notes: '', review_event: 1, conditions: { roles: [], stages: [], paths: [] }, targets: [] },
    source: { author: 'a0', artifact_id: 'art1', artifact_version: 1, artifact_digest: 'digest', review_event: 1, activation: 0 } })
  vi.spyOn(api, 'artifactContentAt').mockResolvedValue('Reviewed proposal')
  const rollback = vi.spyOn(api, 'rollbackProposal').mockResolvedValue(undefined)
  const host = document.createElement('div'); const root = createRoot(host)
  await act(async () => root.render(<ArtifactTab path="props/lesson.md" />))
  const button = [...host.querySelectorAll('button')].find((b) => b.textContent === i18n.t('experience.withdraw'))!
  expect(button.title).toContain(i18n.t('experience.withdraw'))
  await act(async () => button.click())
  expect(rollback).not.toHaveBeenCalled()
  expect(askConfirm.mock.calls[0][0].body).toBe(i18n.t('experience.withdrawBody'))
  await act(async () => { await askConfirm.mock.calls[0][0].run() })
  expect(rollback).toHaveBeenCalledWith('experience-prop')
  expect(host.textContent).not.toContain(i18n.t('experience.withdraw'))
  await act(async () => root.unmount())
  useUiStore.setState(saved, true)
  vi.restoreAllMocks()
})
