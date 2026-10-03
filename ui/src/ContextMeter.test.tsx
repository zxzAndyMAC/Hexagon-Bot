import { mockTimelineFacts } from './timelineMock'
import { afterEach, expect, it } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { ContextMeter } from './components/ContextMeter'
import { useUiStore } from './store'
import i18n from './i18n'
import type { TimelineItem } from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(() => { root?.unmount(); el.replaceChildren(); el.remove() })
// Issue15: occupancy comes from complete facts even with an empty visible window.
it('shows latest request occupancy, not accumulated billing, and supports keyboard disclosure', async () => {
  await i18n.changeLanguage('zh-CN')
  const event = (id: number, used: number): TimelineItem => ({ message: null, event: {
    id, project_id: 'p', agent_id: 'a', stage_run_id: null, kind: 'system', created_at: '',
    payload: { kind: 'request_envelope', context: { used_tokens: used, window_tokens: 1_000_000, compact_at_tokens: 800_000, model_slot: 'chat' } },
  } })
  useUiStore.setState({ timeline: [], timelineFacts: mockTimelineFacts([event(1, 900_000), event(2, 100_000)], {expected_project_root:'/p',watched_tool_streams:[],watched_message_ids:[]}), team: [] })
  root = createRoot(el)
  await act(async () => { root.render(<ContextMeter />) })
  // 2026-10-01 owner: details must open on hover/focus, without a click.
  document.body.appendChild(el)
  const trigger = el.querySelector('button')!
  expect(trigger.textContent).toContain('10%')
  expect(el.querySelector('[role=tooltip]')).toBeNull()
  await act(async () => { trigger.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })) })
  expect(el.querySelector('[role=tooltip]')).not.toBeNull()
  expect(trigger.getAttribute('aria-label')).toBe('上下文用量')
  expect(el.textContent).toContain('800,000')
  expect(el.textContent).toContain('不是累计用量')
  await act(async () => { trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
  expect(el.querySelector('[role=tooltip]')).toBeNull()
  await act(async () => { trigger.focus() })
  expect(el.querySelector('[role=tooltip]')).not.toBeNull()
  await act(async () => { useUiStore.setState({ timeline: [], timelineFacts: null }) })
  expect(trigger.textContent).toContain('—')
  expect(el.textContent).toContain('首次模型请求后显示')
})

it('shows the addressed role even when another role has the latest request', async () => {
  const event = (id: number, agent: string, used: number): TimelineItem => ({ message: null, event: {
    id, project_id: 'p', agent_id: agent, stage_run_id: null, kind: 'system', created_at: '2026-10-01 06:00:00',
    payload: { kind: 'request_envelope', context: { used_tokens: used, window_tokens: 1_000_000, compact_at_tokens: 800_000, model_slot: 'chat' } },
  } })
  useUiStore.setState({ timeline: [], timelineFacts: mockTimelineFacts([event(1, 'a', 100_000), event(2, 'b', 800_000)], {expected_project_root:'/p',watched_tool_streams:[],watched_message_ids:[]}), team: [
    { id: 'a', role: 'QA', status: 'active', model_slot: 'chat', avatar_hash: null },
    { id: 'b', role: '前端', status: 'active', model_slot: 'chat', avatar_hash: null },
  ] })
  root = createRoot(el)
  await act(async () => { root.render(<ContextMeter agentIds={['a']} />) })
  expect(el.querySelector('button')?.textContent).toContain('10%')
  await act(async () => { el.querySelector('button')!.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })) })
  expect(el.querySelector('[role=tooltip]')?.textContent).toContain('QA')
  expect(el.querySelector('time')?.getAttribute('dateTime')).toBe('2026-10-01T06:00:00.000Z')
  await act(async () => { root.render(<ContextMeter agentIds={['never-run']} />) })
  expect(el.querySelector('button')?.textContent).toContain('—')
  expect(el.textContent).toContain('首次模型请求后显示')
})
