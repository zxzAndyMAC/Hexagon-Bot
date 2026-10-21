import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api } from './api'
import { useUiStore } from './store'
import { CommandPalette } from './components/CommandPalette'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it.each(['检查', '测试', '检验'])('lets a Chinese owner find and execute quality checks by %s', async (query) => {
  const language = i18n.language
  const saved = useUiStore.getState()
  const recents = localStorage.getItem('hexagon.palette.recents')
  await i18n.changeLanguage('zh-CN')
  useUiStore.setState({ team: [], artifacts: [], pending: [], invalidate: async () => {} })
  localStorage.removeItem('hexagon.palette.recents')
  const run = vi.spyOn(api, 'runChecks').mockResolvedValue({ results: [] })
  const el = document.createElement('div')
  document.body.append(el)
  const root = createRoot(el)
  try {
    await act(async () => root.render(<CommandPalette onClose={() => {}} />))
    // ui-redesign 票 07（components D18）：面板换成 Command in Dialog，内容挂在 body 的 portal 里，按 document 查；
    // 行上多了灰描述「测试 · 性能 · 可访问性」，所以名字断言改为包含。
    const input = document.querySelector<HTMLInputElement>('[data-command-palette] input')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, query)
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    const option = document.querySelector('[role="option"]')
    expect(option?.textContent).toContain('跑检验')
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    expect(run).toHaveBeenCalledExactlyOnceWith()
  } finally {
    await act(async () => root.unmount())
    el.remove()
    vi.restoreAllMocks()
    useUiStore.setState(saved, true)
    await i18n.changeLanguage(language)
    if (recents === null) localStorage.removeItem('hexagon.palette.recents')
    else localStorage.setItem('hexagon.palette.recents', recents)
  }
})

// owner-followups-1015 #11（C12a）：会话卡收成胶囊后，「稍后继续…」常驻入口在 ⌘K（同头部 ⋯ 菜单）；
// 做不了时灰显写原因，可做时显示 sessionScheduleOnce 的当前键位，执行即打开挂在会话状态槽上的时间弹层。
it('lists Continue later… with its binding, disabled with a reason once a booking exists', async () => {
  const saved = useUiStore.getState()
  const session = {
    session_id: 's', project_id: 'p', project_root: '/p', baseline: 'main', status: 'running' as const, current_turn_id: null, worktree: null,
    worktree_intent: null, runtime: null, reconnect_attempts: 0, reconnect_budget: 3, is_current: true, created_at: '', updated_at: '',
    last_branch: null, last_reason_code: null, last_elapsed_ms: 0,
  }
  const summary = { session, recovery_run_id: null, pending_count: 0, highest_pending_severity: null, recent_diagnostics: [], capabilities: [], triggers: [], next_action: null }
  useUiStore.setState({ team: [], artifacts: [], pending: [], invalidate: async () => {}, sessionSummary: summary, sessionLaterOpen: false })
  vi.spyOn(api, 'releaseTargets').mockResolvedValue([])
  vi.spyOn(api, 'pushTargets').mockRejectedValue(new Error('none'))
  const el = document.createElement('div')
  document.body.append(el)
  const root = createRoot(el)
  const row = () => document.querySelector('[data-palette-item="c-session-later"]')!
  try {
    await act(async () => root.render(<CommandPalette onClose={() => {}} />))
    expect(row().textContent).toContain('Continue later…')
    expect(row().getAttribute('aria-disabled')).not.toBe('true')
    await act(async () => { (row() as HTMLElement).click() })
    expect(useUiStore.getState().sessionLaterOpen).toBe(true)
    await act(async () => root.unmount())
    useUiStore.setState({ sessionLaterOpen: false, sessionSummary: { ...summary, triggers: [{
      trigger_id: 't', session_id: 's', project_root: '/p', baseline: 'main', model_slot: 'default', permission_package: [], fire_at: 1,
      trace_id: 'x', state: 'scheduled', created_at: '', consumed_at: null,
    }] } })
    const again = createRoot(el)
    await act(async () => again.render(<CommandPalette onClose={() => {}} />))
    expect(row().getAttribute('aria-disabled')).toBe('true')
    expect(row().textContent).toContain('Already booked')
    await act(async () => again.unmount())
  } finally {
    el.remove()
    vi.restoreAllMocks()
    useUiStore.setState(saved, true)
  }
})
