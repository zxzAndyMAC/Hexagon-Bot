// ui-redesign 票 05（components C12、interaction §7）：归来摘要钉在对话顶部，
// 「需要你处理」按 SEVERITY 排、首行带顶卡键位；建议下一步只填入输入条，绝不发送。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import App from './App'
import { api, type PendingQuestion, type TimelineItem } from './api'
import { useUiStore } from './store'
import { bindingFor, formatBinding } from './keymap'
import i18n from './i18n'
import { MotionGlobalConfig } from 'motion/react'

vi.mock('echarts/core', () => ({ use: vi.fn(), init: () => ({ setOption: vi.fn(), resize: vi.fn(), dispose: vi.fn() }) }))
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
// 待决坞（PendingCards）的入场动画在 happy-dom 里被取消会抛 AbortError；这里只测摘要，跳过动画。
MotionGlobalConfig.skipAnimations = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()
// 票 03（ADR 0088）：时间线不再是页签，夹具只清空画布。

const summary: TimelineItem = {
  event: { id: 20, kind: 'return_summary', project_id: 'p1', agent_id: null, stage_run_id: null, created_at: '2026-10-13 14:02:00',
    payload: { since_event: 0, deliveries: [{ path: 'src/TodoList.tsx', kind: 'code' }], reviews: { passed: 3, rejected: 1 },
      flags: { submitted: 0, adjudicated: 0, escalated: 0 }, permissions: { asked: 0, allowed: 0, denied: 0 },
      stages: { finished: 1, skipped: 0, rewound: 0 }, pending_todos: [], attention: { unresolved_actions: 0, unknown_cost_records: 0, budget_stops: 0, exception_decisions: 0, policy_candidates: 0 } } },
  message: null,
}
const pending = [
  { id: 'perm', kind: 'permission', agent_id: 'a1', payload: { tool: 'bash', input: { cmd: 'npx playwright install' } }, state: 'queued' },
  { id: 'pub', kind: 'publish', agent_id: 'a0', payload: { remote: 'origin' }, state: 'queued' },
] as unknown as PendingQuestion[]

beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({ projectRoot: null, projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null, team: [], pending, timeline: [summary], modalScope: 'workbench', intakeDraft: false })
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => root.render(<App />))
})

afterEach(async () => {
  await act(async () => root.unmount())
  el.remove()
  vi.restoreAllMocks()
  useUiStore.setState(initial)
})

it('pins the return summary with severity-ordered attention and the top-card key on the first row', () => {
  const card = el.querySelector('[data-return-summary]')!
  expect(card).toBeTruthy()
  const rows = [...card.querySelectorAll('li')]
  // R-02：发布（SEVERITY 0）排在权限（3）前；首行 = 坞顶卡，带 approve 当前绑定
  expect(rows[0].getAttribute('data-attention')).toBe('irreversible')
  expect(rows[0].querySelector('kbd')?.textContent).toBe(formatBinding(bindingFor('approve')))
  expect(rows[1].getAttribute('data-attention')).toBe('authorize')
  expect(rows[1].querySelector('kbd')).toBeNull()
})

it('a suggested next step only fills the composer and never sends', async () => {
  const send = vi.spyOn(api, 'sendMessage').mockResolvedValue(undefined as never)
  const suggestion = el.querySelector<HTMLButtonElement>('[data-suggestion]')!
  const text = suggestion.textContent!
  expect(text).toBeTruthy()
  await act(async () => suggestion.click())
  const input = el.querySelector<HTMLTextAreaElement>('#composer-input')!
  expect(input.value).toBe(text)
  expect(document.activeElement).toBe(input)
  expect(send).not.toHaveBeenCalled()
  // 已有草稿时追加，不覆盖负责人写了一半的话
  await act(async () => suggestion.click())
  expect(input.value).toBe(`${text}\n${text}`)
  expect(send).not.toHaveBeenCalled()
})

it('collapses to a single line and expands back', async () => {
  const collapse = el.querySelector<HTMLButtonElement>(`[data-return-summary] button[aria-label="${i18n.t('returnSummary.collapse')}"]`)!
  await act(async () => collapse.click())
  const line = el.querySelector<HTMLButtonElement>('[data-return-summary="collapsed"]')!
  expect(line.textContent).toContain('2 waiting for you')
  await act(async () => line.click())
  expect(el.querySelector('[data-return-summary] li')).toBeTruthy()
})

// 票 04（components C12 禁用态，补设计）：处理掉的行划除并转 ink-3，保留 30 秒可读后移除；首行键位交给下一张顶卡。
it('a handled row is struck through for 30 seconds, then removed', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] })
  try {
    await act(async () => { useUiStore.setState({ pending: [pending[0]] }) })
    const card = el.querySelector('[data-return-summary]')!
    let rows = [...card.querySelectorAll('li')]
    expect(rows.map((r) => r.hasAttribute('data-handled'))).toEqual([false, true])
    expect(rows[0].querySelector('kbd')?.textContent).toBe(formatBinding(bindingFor('approve')))
    expect(rows[1].querySelector('kbd')).toBeNull()
    await act(async () => { vi.advanceTimersByTime(29_000) })
    expect(card.querySelectorAll('li[data-handled]')).toHaveLength(1)
    await act(async () => { vi.advanceTimersByTime(1_100) })
    rows = [...card.querySelectorAll('li')]
    expect(rows).toHaveLength(1)
    expect(rows[0].hasAttribute('data-handled')).toBe(false)
  } finally { vi.useRealTimers() }
})
