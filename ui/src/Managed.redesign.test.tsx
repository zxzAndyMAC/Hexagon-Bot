// ui-redesign 票 08：托管细条（出现与跳转）、托管决定行、归来时托管报告置顶、managedView 纯逻辑。
import { act, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api, type TimelineItem } from './api'
import { useUiStore } from './store'
import { ManagedStrip } from './components/ManagedStrip'
import { Timeline } from './components/Timeline'
import { buildRows } from './timelineModel'
import { delegationSince, latestDecisionEvent, rowChip } from './managedView'
import type { DelegationStatus } from './gen/DelegationStatus'

vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent }: { data: unknown[]; itemContent: (i: number, row: unknown) => ReactNode }) =>
    <div>{data.map((r, i) => <div key={i}>{itemContent(i, r)}</div>)}</div>,
}))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const on: DelegationStatus = { enabled: true, paused: false, generation: 1, delegation_id: 'g1' }
const el = document.createElement('div')
let root: ReturnType<typeof createRoot> | null = null

const sys = (id: number, payload: Record<string, unknown>, kind = 'system', created_at = '2026-10-14 08:00:00'): TimelineItem =>
  ({ event: { id, kind, agent_id: null, stage_run_id: null, project_id: 'p1', created_at, payload }, message: null }) as TimelineItem
const decision = (id: number, card: string, action = 'allow') =>
  sys(id, { kind: 'managed_decision', card_id: card, card_kind: 'permission', action, reason: 'project network task', via: 'model', outcome: 'allowed', actor: 'managed:g1:1' })

beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({ projectRoot: '/proj', timeline: [], pending: [], team: [], streams: {}, thinkings: {}, streamDone: {}, toasts: [] })
  vi.spyOn(api, 'managedExternalOps').mockResolvedValue([])
})
afterEach(async () => {
  if (root) await act(async () => root!.unmount())
  root = null; el.replaceChildren(); el.remove(); vi.restoreAllMocks()
})

async function mount(node: ReactNode) {
  document.body.appendChild(el); root = createRoot(el)
  await act(async () => root!.render(node))
}

it('the strip appears only while delegation is active with items, and jumps to the decision row', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([
    { id: 'q1', kind: 'permission', agent_id: 'a0', payload: {}, state: 'queued' },
    { id: 'q2', kind: 'publish', agent_id: 'a0', payload: {}, state: 'queued' },
  ])
  vi.spyOn(api, 'managedHistory').mockResolvedValue([])
  useUiStore.setState({ timeline: [decision(40, 'q1'), decision(41, 'q9')] })
  const jumped: number[] = []
  const listen = (e: Event) => jumped.push((e as CustomEvent<number>).detail)
  window.addEventListener('hexagon:timeline-jump', listen)
  try {
    await mount(<ManagedStrip />)
    const strip = el.querySelector('[data-managed-strip]')!
    expect(strip.textContent).toContain('Delegation is handling 2 item(s)')
    const view = [...strip.querySelectorAll('button')].find(b => b.textContent === 'View')!
    await act(async () => view.click())
    expect(jumped).toEqual([40])
  } finally { window.removeEventListener('hexagon:timeline-jump', listen) }
})

it('the strip yields when paused or when nothing is being processed', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue({ ...on, paused: true })
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([{ id: 'q1', kind: 'permission', agent_id: 'a0', payload: {}, state: 'queued' }])
  vi.spyOn(api, 'managedHistory').mockResolvedValue([])
  await mount(<ManagedStrip />)
  expect(el.querySelector('[data-managed-strip]')).toBeNull()
})

it('renders a managed decision as an authored row with the host-classified result', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue({ ...on, enabled: false })
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([])
  vi.spyOn(api, 'managedHistory').mockResolvedValue([
    { event_id: 40, card_id: 'q1', card_kind: 'permission', action: 'allow', reason: 'project network task', via: 'model', outcome: 'allowed', actor: 'managed:g1:1', result: 'done' },
  ])
  // Three in a row would fold as system events; managed decisions must not.
  // projectRoot null → Timeline reads the store's legacy timeline instead of a host window page.
  useUiStore.setState({ projectRoot: null, timeline: [decision(40, 'q1'), decision(41, 'q2', 'deny'), decision(42, 'q3')] })
  await mount(<Timeline />)
  const rows = el.querySelectorAll('[data-managed-row]')
  expect(rows).toHaveLength(3)
  const first = el.querySelector('[data-managed-row="40"]')!
  expect(first.textContent).toContain('Delegation')
  expect(first.textContent).toContain('Allowed')
  expect(first.textContent).toContain('Reason: project network task')
  expect(first.textContent).toContain('Executed')
  // Not in the history read → no guessed chip.
  expect(el.querySelector('[data-managed-row="41"]')!.getAttribute('data-managed-result')).toBeNull()
})

it('pins the delegation report instead of the return summary while delegating', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([])
  vi.spyOn(api, 'managedHistory').mockResolvedValue([
    { event_id: 40, card_id: 'q1', card_kind: 'publish', action: 'deny', reason: 'no_target', via: 'host', outcome: 'not_published', actor: 'managed:g1:1', result: 'incomplete' },
    { event_id: 41, card_id: 'q2', card_kind: 'permission', action: 'allow', reason: 'ok', via: 'model', outcome: 'allowed', actor: 'managed:g1:1', result: 'done' },
  ])
  useUiStore.setState({ projectRoot: null, timeline: [decision(40, 'q1'), sys(50, { away_secs: 1200 }, 'return_summary')] })
  await mount(<Timeline />)
  const pin = el.querySelector('.tl-pin')!
  expect(pin.querySelector('[data-managed-report]')).not.toBeNull()
  // Unfinished first, with its next step.
  const groups = [...pin.querySelectorAll('[data-managed-unfinished], [data-managed-done]')].map(g => g.getAttribute('data-managed-unfinished') !== null ? 'u' : 'd')
  expect(groups).toEqual(['u', 'd'])
  expect(pin.querySelector('[data-managed-unfinished]')!.textContent).toContain('Show in timeline')
})

it('managedView maps host results without guessing from outcome text', () => {
  expect(rowChip({ result: 'failed', action: 'allow' })).toBe('failed')
  expect(rowChip({ result: 'incomplete', action: 'allow' })).toBe('incomplete')
  expect(rowChip({ result: 'done', action: 'deny' })).toBe('refused')
  expect(rowChip({ result: 'done', action: 'allow' })).toBe('executed')
  expect(rowChip({ result: 'done', action: 'accept' })).toBe('effective')
  const tl = [sys(1, { kind: 'delegation_changed', enabled: true }, 'system', '2026-10-14 08:00:00'), decision(2, 'q1'), decision(3, 'q1')]
  expect(delegationSince(tl)?.toISOString()).toBe('2026-10-14T08:00:00.000Z')
  expect(delegationSince([...tl, sys(4, { kind: 'delegation_changed', enabled: false })])).toBeNull()
  expect(latestDecisionEvent(tl, 'q1')).toBe(3)
  expect(buildRows([decision(1, 'a'), decision(2, 'b'), decision(3, 'c')], 'decisions')).toHaveLength(3)
})
