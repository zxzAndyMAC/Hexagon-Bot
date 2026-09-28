import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import './i18n'
import i18n from 'i18next'
import { api } from './api'
import { ExperienceHistory } from './components/ExperienceHistory'
import type { ExperienceHistoryPage } from './gen/ExperienceHistoryPage'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
afterEach(() => vi.restoreAllMocks())
it('pages, retries stale history, and explicitly selects a related entry', async () => {
  const query = vi.spyOn(api, 'experienceHistory').mockResolvedValue({ items: [], next_cursor: 'cursor1' })
  const related = vi.fn()
  const host = document.createElement('div'); const root = createRoot(host)
  await act(async () => root.render(<ExperienceHistory projectRoot="/p" skill="alpha" entryId="e1" onRelated={related} />))
  expect(host.textContent).toContain(i18n.t('experience.emptyHistory'))
  const button = (key: string) => [...host.querySelectorAll('button')].find((b) => b.textContent === i18n.t(key))!
  query.mockRejectedValueOnce(new Error('stale cursor'))
  await act(async () => button('experience.nextPage').click())
  expect(query).toHaveBeenLastCalledWith(expect.objectContaining({ project_root: '/p', cursor: 'cursor1' }))
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('stale cursor')
  await act(async () => button('experience.refreshHistory').click())
  expect(query).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: null }))
  query.mockResolvedValue({ items: [{ kind: 'related', skill: 'beta', entry_id: 'e2', revision: 1, state: 'revoked' }], next_cursor: null })
  await act(async () => { const select = host.querySelector('select')!; select.value = 'related'; select.dispatchEvent(new Event('change', { bubbles: true })) })
  expect(host.textContent).toContain(i18n.t('experience.revoked'))
  expect(related).not.toHaveBeenCalled()
  await act(async () => button('experience.openRelated').click())
  expect(related).toHaveBeenCalledWith('beta', 'e2')
  await act(async () => root.unmount())
})
it('discards late pages after project scope changes', async () => {
  let finish!: (page: ExperienceHistoryPage) => void
  vi.spyOn(api, 'experienceHistory').mockReturnValueOnce(new Promise((resolve) => { finish = resolve })).mockResolvedValue({ items: [], next_cursor: null })
  const host = document.createElement('div'); const root = createRoot(host)
  const related = vi.fn()
  await act(async () => root.render(<ExperienceHistory projectRoot="/old" skill="alpha" entryId="e1" onRelated={related} />))
  await act(async () => root.render(<ExperienceHistory projectRoot="/new" skill="alpha" entryId="e1" onRelated={related} />))
  await act(async () => finish({ items: [{ kind: 'related', skill: 'OLD_PROJECT_ONLY', entry_id: 'old', revision: 1, state: 'active' }], next_cursor: 'old-cursor' }))
  expect(host.textContent).not.toContain('OLD_PROJECT_ONLY')
  expect([...host.querySelectorAll('button')].find((b) => b.textContent === i18n.t('experience.nextPage'))?.disabled).toBe(true)
  await act(async () => root.unmount())
})
it('keeps the last selected source when source reads finish in reverse order', async () => {
  const sources = [1, 2].map((review_event) => ({ review_event, activation: 1, artifact_id: `art${review_event}`, author: 'a0', artifact_version: 1, artifact_digest: 'digest' }))
  vi.spyOn(api, 'experienceHistory').mockResolvedValue({ items: sources.map((source) => ({ kind: 'source', source, path: `${source.artifact_id}.md` })), next_cursor: null })
  let finish!: (value: Awaited<ReturnType<typeof api.experienceSourceDocument>>) => void
  vi.spyOn(api, 'experienceSourceDocument').mockReturnValueOnce(new Promise((resolve) => { finish = resolve })).mockResolvedValue({ source: sources[1], path: 'art2.md', content: 'LATEST_SELECTION' })
  const host = document.createElement('div'); const root = createRoot(host)
  await act(async () => root.render(<ExperienceHistory projectRoot="/p" skill="alpha" entryId="e1" onRelated={vi.fn()} />))
  const buttons = [...host.querySelectorAll('button')].filter((b) => b.textContent === i18n.t('experience.openSource'))
  await act(async () => buttons[0].click())
  await act(async () => buttons[1].click())
  await act(async () => finish({ source: sources[0], path: 'art1.md', content: 'STALE_SELECTION' }))
  expect(host.textContent).toContain('LATEST_SELECTION')
  expect(host.textContent).not.toContain('STALE_SELECTION')
  await act(async () => root.unmount())
})
