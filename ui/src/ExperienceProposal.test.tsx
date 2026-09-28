import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import './i18n'
import { api } from './api'
import { ExperienceProposal } from './components/ExperienceProposal'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
afterEach(() => vi.restoreAllMocks())

it('shows the host source and explicit target alongside conditions without granting approval', async () => {
  vi.spyOn(api, 'experienceProposal').mockResolvedValue({
    proposal_id: 'prop1', previous_entries: [], legacy: null, recovery_pending: false,
    request: { create_role_skill: false, body: 'Check parser input', notes: 'Only for text parsing', review_event: 12,
      conditions: { roles: ['Parser'], stages: ['review'], paths: ['src/parser'] },
      targets: [{ skill: 'parser-skill', expected_digest: 'abc', reason: 'Owns parsing guidance' }] },
    source: { author: 'author-1', artifact_id: 'artifact-1', artifact_version: 2, artifact_digest: '123', review_event: 12, activation: 10 },
  })
  const host = document.createElement('div')
  const root = createRoot(host)
  await act(async () => { root.render(<ExperienceProposal proposalId="prop1" />) })
  expect(host.textContent).toContain('Check parser input')
  expect(host.textContent).toContain('parser-skill')
  expect(host.textContent).toContain('Owns parsing guidance')
  expect(host.textContent).toContain('author-1')
  expect(host.textContent).toContain('artifact-1')
  expect(host.textContent).toContain('src/parser')
  expect(host.querySelector('button')).toBeNull()
  await act(async () => root.unmount())
})
