// gap A2–A5（2026-10-15）：托管时段、预算 / 花费、报告导出、范围 / 证据分类——全部取宿主事实。
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { ManagedPill } from './components/ManagedPill'
import { ManagedReport } from './components/ManagedReport'
import { api } from './api'
import { useUiStore } from './store'
import i18n from './i18n'
import { rowChip, sessionMinutes } from './managedView'
import { managedReportMarkdown } from './managedReportExport'
import type { DelegationStatus } from './gen/DelegationStatus'
import type { ManagedRecord } from './gen/ManagedRecord'
import type { ManagedSummary } from './gen/ManagedSummary'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
const on: DelegationStatus = { enabled: true, paused: false, generation: 1, delegation_id: 'g1' }
const sqlTime = (msAgo: number) => new Date(Date.now() - msAgo).toISOString().slice(0, 19).replace('T', ' ')
const summary = (over: Partial<ManagedSummary> = {}): ManagedSummary => ({
  started_at: sqlTime(42 * 60_000 + 5_000), ended_at: null, limit_cents: 10_000, spent_mc: 4_540_000,
  session_spent_mc: 680_000, cost_unknown: false, ...over,
})
const record = (over: Partial<ManagedRecord>): ManagedRecord => ({
  event_id: 1, card_id: 'q1', card_kind: 'permission', action: 'allow', reason: 'r', via: 'model', outcome: 'allowed',
  actor: 'managed:g1:1', result: 'done', scope: 'in_scope', evidence: 'model_choice', ...over,
})

beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({ projectRoot: '/proj' })
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'approvalMode').mockResolvedValue({ project_id: 'p1', project_root: '/proj', mode: 'assisted' })
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([])
  vi.spyOn(api, 'managedExternalOps').mockResolvedValue([])
})
afterEach(async () => { await act(async () => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })

async function mount(node: React.ReactNode) {
  document.body.appendChild(el); root = createRoot(el)
  await act(async () => root.render(node))
}

it('rowChip shows out-of-scope from the host classification ahead of the result', () => {
  expect(rowChip({ result: 'incomplete', action: 'incomplete', scope: 'out_of_scope' })).toBe('out_of_scope')
  expect(rowChip({ result: 'incomplete', action: 'incomplete', scope: 'in_scope' })).toBe('incomplete')
})

it('sessionMinutes counts from the host start to the end or now', () => {
  expect(sessionMinutes(null, null)).toBeNull()
  expect(sessionMinutes('2026-10-15 14:20:00', '2026-10-15 15:02:30')).toBe(42)
})

it('pill popover shows elapsed minutes and the project budget', async () => {
  vi.spyOn(api, 'managedSummary').mockResolvedValue(summary())
  vi.spyOn(api, 'managedHistory').mockResolvedValue([])
  await mount(<ManagedPill />)
  await act(async () => el.querySelector<HTMLButtonElement>('.managed-pill')!.click())
  const menu = document.querySelector('[data-managed-menu]')!
  expect(menu.querySelector('[data-managed-elapsed]')!.textContent).toBe('42 min')
  const budget = menu.querySelector('[data-managed-budget]')!
  expect(budget.textContent).toContain('Budget')
  expect(budget.textContent).toMatch(/45\.40.*100\.00/)
  expect(budget.querySelector('[data-slot="progress"]')).not.toBeNull()
})

it('without a project limit the budget row shows spend only, no bar', async () => {
  vi.spyOn(api, 'managedSummary').mockResolvedValue(summary({ limit_cents: null }))
  vi.spyOn(api, 'managedHistory').mockResolvedValue([])
  await mount(<ManagedPill />)
  await act(async () => el.querySelector<HTMLButtonElement>('.managed-pill')!.click())
  const budget = document.querySelector('[data-managed-budget]')!
  expect(budget.textContent).toContain('no limit')
  expect(budget.querySelector('[data-slot="progress"]')).toBeNull()
})

it('report header shows the session, spend KPI replaces failed, evidence comes from the host, export writes markdown', async () => {
  vi.spyOn(api, 'managedSummary').mockResolvedValue(summary({ cost_unknown: true }))
  vi.spyOn(api, 'managedHistory').mockResolvedValue([
    record({ event_id: 3, action: 'incomplete', reason: 'target_missing', via: 'host', result: 'incomplete', scope: 'out_of_scope', evidence: 'host_rule', card_kind: 'publish' }),
    record({ event_id: 2, action: 'accept', reason: 'evidence_complete', via: 'host', result: 'done', evidence: 'real_check', card_kind: 'stamp' }),
    record({ event_id: 1, result: 'failed', outcome: 'failed: x' }),
  ])
  const blobs: Blob[] = []
  const created = vi.fn((blob: Blob) => { blobs.push(blob); return 'blob:x' })
  Object.assign(URL, { createObjectURL: created, revokeObjectURL: () => {} })
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
  await mount(<ManagedReport card onClose={() => {}} />)
  const report = el.querySelector('[data-managed-report]')!
  expect(report.querySelector('[data-managed-period]')!.textContent).toMatch(/· 42 min$/)
  const kpi = report.querySelector('[data-managed-kpi]')!.textContent!
  expect(kpi).toContain('Incomplete2')
  expect(kpi).toContain('Spend')
  expect(kpi).toMatch(/At least .*6\.80/)
  expect(kpi).not.toContain('Failed')
  expect(report.querySelector('[data-evidence="real_check"]')!.textContent).toBe('Real checks')
  const exportButton = [...report.querySelectorAll('button')].find(b => b.textContent === 'Export')!
  expect(exportButton.title).toBe('Export delegation report')
  await act(async () => exportButton.click())
  expect(created).toHaveBeenCalledTimes(1)
  const markdown = await blobs[0].text()
  expect(markdown).toMatch(/^# Delegation report/)
  expect(markdown).toContain('Out of scope')
  expect(markdown).toContain('Real checks')
})

it('markdown escapes table pipes and states an empty external section', () => {
  const text = managedReportMarkdown(i18n.t.bind(i18n) as never, {
    title: 'R', period: null, records: [record({ reason: 'a|b', result: 'incomplete' })], external: [], summary: null,
    recordTitle: () => 'x|y',
  })
  expect(text).toContain('x\\|y')
  expect(text).toContain(i18n.t('delegation.externalEmpty'))
})
