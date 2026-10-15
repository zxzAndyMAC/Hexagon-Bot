// 功能缺口 C3（票 13 偏差 5；components A19）：剩余纯文本空态补「为什么空 + 一个动作」，
// 没有可给的动作时说明写明「不需要你做什么」。
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { api } from './api'
import { useUiStore } from './store'
import i18n from './i18n'
import { SettingsPage } from './components/SettingsPage'
import { ExperienceHistory } from './components/ExperienceHistory'
import { ManagedReport } from './components/ManagedReport'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const el = document.createElement('div')
let root: ReturnType<typeof createRoot> | undefined

beforeEach(async () => { await i18n.changeLanguage('en'); document.body.appendChild(el) })
afterEach(async () => {
  if (root) await act(async () => root!.unmount())
  root = undefined; el.replaceChildren(); el.remove(); vi.restoreAllMocks()
})
const mount = async (node: React.ReactNode) => { root = createRoot(el); await act(async () => root!.render(node)) }

it('data boundary: an empty recipient row offers the settings section that fills it', async () => {
  vi.spyOn(api, 'dataBoundary').mockResolvedValue({ credential_backend: 'keychain', project_open: false, recipients: [] })
  await mount(<SettingsPage projectless onBack={() => {}} />)
  expect(el.textContent).toContain(i18n.t('dataBoundary.empty'))
  expect(el.querySelector('[data-boundary-go="mcp"]')?.textContent).toBe(i18n.t('dataBoundary.addMcp'))
  await act(async () => el.querySelector<HTMLButtonElement>('[data-boundary-go="models"]')!.click())
  expect(el.querySelector('[data-data-boundary]')).toBeNull() // 已离开「通用」分区
})

it('experience history: empty explains why for the selected list and refreshes', async () => {
  const query = vi.spyOn(api, 'experienceHistory').mockResolvedValue({ items: [], next_cursor: null })
  await mount(<ExperienceHistory projectRoot="/p" skill="alpha" entryId="e1" onRelated={() => {}} />)
  const empty = el.querySelector('[data-empty]')!
  expect(empty.textContent).toContain(i18n.t('experience.emptyHistoryBody_sources'))
  query.mockClear()
  await act(async () => empty.querySelector('button')!.click())
  expect(query).toHaveBeenCalledWith(expect.objectContaining({ cursor: null }))
})

it('managed report: no decisions yet is an A19 empty that says nothing is needed', async () => {
  useUiStore.setState({ projectRoot: '/proj' })
  vi.spyOn(api, 'projectDelegation').mockResolvedValue({ enabled: true, paused: false, generation: 1, delegation_id: 'g1' })
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([])
  vi.spyOn(api, 'managedExternalOps').mockResolvedValue([])
  vi.spyOn(api, 'managedHistory').mockResolvedValue([])
  vi.spyOn(api, 'managedSummary').mockResolvedValue({ started_at: null, ended_at: null, limit_cents: null, spent_mc: 0, session_spent_mc: 0, cost_unknown: false })
  await mount(<ManagedReport />)
  const empty = el.querySelector('[data-managed-empty]')!
  expect(empty.textContent).toContain(i18n.t('delegation.empty'))
  expect(empty.textContent).toContain(i18n.t('delegation.emptyBody'))
})
