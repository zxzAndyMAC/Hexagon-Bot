import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { defaultWorktreePath } from './components/SessionRecoveryStrip'
import { SessionPill } from './components/SessionPill'
import { TooltipProvider } from '@/components/ui/tooltip'
import { useUiStore } from './store'
import { isMac } from './keymap'
import type { SessionSummary } from './gen/SessionSummary'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const base: SessionSummary = {
  session: {
    session_id: 'session-1a2b3c4d5e', project_id: 'p', project_root: '/work/demo', baseline: 'main', status: 'paused',
    current_turn_id: 'run-1', worktree: null, worktree_intent: null, runtime: null, reconnect_attempts: 0, reconnect_budget: 3,
    is_current: true, created_at: '', updated_at: '', last_branch: null, last_reason_code: 'owner_paused', last_elapsed_ms: 0,
  },
  recovery_run_id: 'run-1', pending_count: 1, highest_pending_severity: 'critical', recent_diagnostics: [], capabilities: [], triggers: [], next_action: 'resume_session',
}

const trigger = (state: 'expired' | 'scheduled') => ({
  trigger_id: 'later-1', session_id: 'session-1a2b3c4d5e', project_root: '/work/demo', baseline: 'main', model_slot: 'default',
  permission_package: [], fire_at: 1_800_000_000, trace_id: 'turn:1', state, created_at: '', consumed_at: null,
})

// owner-followups-1015 #11（C12a「改为头部胶囊」）：原「恢复条」用例改为点开胶囊后对弹层断言——
// 卡内容与动作不变，只是外壳从钉顶的 raised 卡换成 A16 弹层（portal 到 body，所以按 document 查）。
describe('SessionHost session pill card (C12a/C12b)', () => {
  let root: ReturnType<typeof createRoot> | undefined
  let el: HTMLDivElement
  const mount = async () => {
    await act(async () => {
      root = createRoot(el)
      root.render(<TooltipProvider><SessionPill /></TooltipProvider>)
    })
  }
  const openCard = async () => {
    const pill = el.querySelector<HTMLButtonElement>('[data-session-pill]')!
    await act(async () => { pill.click() })
  }
  const render = async (summary: SessionSummary) => {
    useUiStore.setState({ sessionSummary: summary })
    await mount()
    await openCard()
  }
  // 只数弹层里的按钮：胶囊本身的名字（「Session · Reconnecting」）不是卡内动作
  const buttons = () => [...document.querySelectorAll('button:not([data-session-pill])')].map((b) => b.textContent ?? '')
  beforeEach(() => {
    el = document.createElement('div')
    document.body.appendChild(el)
    useUiStore.setState({ modalScope: 'workbench', sessionError: null, pending: [], refreshFast: async () => {} })
  })
  afterEach(async () => {
    await act(async () => root?.unmount())
    el.remove()
    vi.restoreAllMocks()
    await i18n.changeLanguage('en')
  })

  it('footer wraps actions onto their own row instead of squeezing next-action text (ticket 16c fr smoke)', async () => {
    await i18n.changeLanguage('fr')
    await render(base)
    const footer = document.querySelector('[data-session-footer]')!
    expect(footer.className).toContain('flex-wrap')
    // 「Rien à faire」/ 原因码曾被三颗动作挤成两行与省略号
    const [label, value] = [...footer.querySelectorAll(':scope > span')]
    expect(label.className).toContain('whitespace-nowrap')
    expect(value.className).toContain('whitespace-nowrap')
    expect(footer.querySelector('[class*="shrink-0"]')?.textContent).toBe('owner_paused')
  })

  it('renders status, turn, pending tier and the resume action in a non-English locale', async () => {
    await i18n.changeLanguage('zh-CN')
    await render(base)
    const text = document.querySelector('[data-session-recovery]')?.textContent ?? ''
    expect(text).toContain('已暂停')
    expect(text).toContain('回合 run-1')
    expect(text).toContain('待决 1 · 最高档')
    expect(text).toContain('owner_paused')
    expect(buttons().some((b) => b.includes('继续会话'))).toBe(true)
    expect(text).not.toContain('session-1a2b3c4d5e')
  })

  it('keeps waiting_user in the pending-card path and shows an expired trigger with Continue', async () => {
    await render({ ...base, session: { ...base.session!, status: 'waiting_user' }, recovery_run_id: null, triggers: [trigger('expired')], next_action: 'review_pending' })
    expect(document.querySelector('[data-session-trigger="expired"]')?.textContent).toContain('Expired')
    expect(buttons().some((b) => b.includes('Resume session'))).toBe(false)
    expect(buttons().some((b) => b === 'Continue')).toBe(true)
    expect(document.body.textContent).toContain('Review pending decisions')
  })

  it('does not offer a second reconnect while the host is already reconnecting', async () => {
    await render({ ...base, session: { ...base.session!, status: 'reconnecting' }, next_action: 'wait_runtime' })
    expect(buttons().some((b) => b.includes('Reconnect'))).toBe(false)
    expect(document.body.textContent).toContain('Wait for runtime')
  })

  it('does not offer reconnect once the budget is exhausted; the next step is a new task', async () => {
    const spy = vi.spyOn(api, 'sessionReconnect').mockResolvedValue(undefined)
    await render({ ...base, session: { ...base.session!, status: 'failed', last_reason_code: 'reconnect_budget_exhausted' }, next_action: 'start_new_task' })
    expect(document.body.textContent).toContain('reconnect_budget_exhausted')
    expect(document.body.textContent).toContain('Send a new instruction to start a new task')
    expect(buttons().some((b) => b.includes('Reconnect'))).toBe(false)
    expect(spy).not.toHaveBeenCalled()
  })

  it('offers reconnect after a failure that still has budget, and calls core', async () => {
    const spy = vi.spyOn(api, 'sessionReconnect').mockResolvedValue(undefined)
    await render({ ...base, session: { ...base.session!, status: 'failed', last_reason_code: 'turn_failed' }, next_action: 'inspect_failure' })
    expect(document.body.textContent).toContain('turn_failed')
    const reconnect = [...document.querySelectorAll('button')].find((b) => b.textContent === 'Reconnect session')!
    await act(async () => { reconnect.click() })
    expect(spy).toHaveBeenCalled()
  })

  it('shows health and proven authorization separately', async () => {
    await render({ ...base, capabilities: [
      { kind: 'mcp', id: 'docs', available: true, granted: false, health: 'unavailable', last_error: 'mcp_worker_exited', requires_user_action: true, binding_hint: 'p' },
      { kind: 'browser', id: 'browser', available: true, granted: true, health: 'healthy', last_error: null, requires_user_action: false, binding_hint: 'p' },
    ] })
    const caps = document.querySelector('[data-session-capabilities]')!
    const mcp = caps.querySelector<HTMLButtonElement>('[data-session-capability-group="mcp"]')!
    const browser = caps.querySelector<HTMLButtonElement>('[data-session-capability-group="browser"]')!
    expect(mcp.getAttribute('data-variant')).toBe('err')
    expect(browser.getAttribute('data-variant')).toBe('ok')
    await act(async () => { mcp.click() })
    const mcpItem = caps.querySelector('[data-session-capability-items="mcp"] [data-slot="badge"]')!
    expect(mcpItem.textContent).toContain('docs · Unavailable')
    expect(mcpItem.querySelector('[aria-label="Authorization proven"]')).toBeNull()
    await act(async () => { browser.click() })
    const browserItem = caps.querySelector('[data-session-capability-items="browser"] [data-slot="badge"]')!
    expect(browserItem.getAttribute('data-variant')).toBe('ok')
    expect(browserItem.querySelector('[aria-label="Authorization proven"]')).not.toBeNull()
  })

  it('continues the first expired trigger from its registered shortcut', async () => {
    const spy = vi.spyOn(api, 'sessionContinueTrigger').mockResolvedValue(trigger('expired'))
    await render({ ...base, triggers: [trigger('expired')] })
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '7', code: 'Digit7', metaKey: isMac, ctrlKey: !isMac, altKey: true, shiftKey: true, bubbles: true }))
    })
    expect(spy).toHaveBeenCalledWith('later-1')
  })

  it('ignores session shortcuts outside the workbench scope', async () => {
    const spy = vi.spyOn(api, 'sessionContinueTrigger').mockResolvedValue(trigger('expired'))
    useUiStore.setState({ modalScope: 'settings' })
    await render({ ...base, triggers: [trigger('expired')] })
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '7', code: 'Digit7', metaKey: isMac, ctrlKey: !isMac, altKey: true, shiftKey: true, bubbles: true }))
    })
    expect(spy).not.toHaveBeenCalled()
  })

  it('requests a cleanup card instead of deleting, and hides the action while a card is pending', async () => {
    const spy = vi.spyOn(api, 'sessionRequestWorktree').mockResolvedValue({ question_id: 'q9', operation: 'cleanup', path: '/work/demo-worktrees/1a2b3c4d', baseline: 'main', baseline_commit: 'abcdef1234', reused: false })
    const withTree = { ...base, session: { ...base.session!, worktree: { path: '/work/demo-worktrees/1a2b3c4d', baseline: 'main', baseline_commit: 'abcdef1234', created_by_session: true, cleanup_requested: false } } }
    await render(withTree)
    const cleanup = [...document.querySelectorAll('button')].find((b) => b.textContent === 'Clean up worktree')!
    await act(async () => { cleanup.click() })
    expect(spy).toHaveBeenCalledWith('cleanup')
    await act(async () => root?.unmount())
    useUiStore.setState({ pending: [{ id: 'q9', kind: 'worktree', agent_id: null, payload: { operation: 'cleanup' }, state: 'queued' }] })
    await render(withTree)
    expect(buttons().includes('Clean up worktree')).toBe(false)
    expect(document.querySelector('[data-session-worktree]')?.textContent).toContain('Awaiting confirmation')
  })

  it('offers only a read-only verify while a worktree outcome is unknown', async () => {
    const spy = vi.spyOn(api, 'sessionVerifyWorktree').mockResolvedValue(base.session!)
    await render({ ...base, session: { ...base.session!, worktree_intent: { operation: 'create', question_id: 'q1', path: '/work/demo-worktrees/x', baseline_commit: 'abc', state: 'executing', reason_code: null } }, next_action: 'verify_worktree' })
    expect(document.body.textContent).toContain('Outcome unknown')
    expect(buttons().some((b) => b.includes('Create worktree'))).toBe(false)
    const verify = [...document.querySelectorAll('button')].find((b) => b.textContent === 'Verify result')!
    await act(async () => { verify.click() })
    expect(spy).toHaveBeenCalled()
  })

  // 票 13（SessionHost 影响审查 2026-10-13）：工作树动作失败走失败 toast，事实漂移写原因码（D8）。
  it('surfaces a worktree drift failure as an error toast carrying worktree_binding_drift', async () => {
    useUiStore.setState({ toasts: [] })
    vi.spyOn(api, 'sessionVerifyWorktree').mockRejectedValue({ code: 'worktree_binding_drift', message: 'worktree HEAD moved' })
    await render({ ...base, session: { ...base.session!, worktree_intent: { operation: 'create', question_id: 'q1', path: '/work/demo-worktrees/x', baseline_commit: 'abc', state: 'executing', reason_code: null } }, next_action: 'verify_worktree' })
    const verify = [...document.querySelectorAll('button')].find((b) => b.textContent === 'Verify result')!
    await act(async () => { verify.click() })
    expect(useUiStore.getState().toasts.at(-1)).toMatchObject({ tone: 'err', code: 'worktree_binding_drift' })
  })

  // Gap D1：触发已到点时 core 拒绝取消，失败 toast 带 trigger_already_fired 原因码与已登记文案。
  it('surfaces a refused cancel as an error toast carrying trigger_already_fired', async () => {
    useUiStore.setState({ toasts: [] })
    vi.spyOn(api, 'sessionCancelTrigger').mockRejectedValue({ code: 'trigger_already_fired', message: 'one-time trigger already fired: later-1' })
    await render({ ...base, session: { ...base.session!, status: 'running' }, triggers: [trigger('scheduled')] })
    expect(buttons().includes('Continue later…')).toBe(false)
    const cancel = [...document.querySelectorAll('button')].find((b) => b.textContent === 'Cancel booking')!
    await act(async () => { cancel.click() })
    expect(useUiStore.getState().toasts.at(-1)).toMatchObject({ tone: 'err', code: 'trigger_already_fired' })
  })

  it('derives the default worktree path outside the project root', () => {
    expect(defaultWorktreePath({ project_root: '/work/demo/', session_id: 'session-1a2b3c4d5e' })).toBe('/work/demo-worktrees/1a2b3c4d')
  })

  it('shows a retryable error card when the summary cannot be read', async () => {
    useUiStore.setState({ sessionError: 'boom' })
    await render({ ...base, session: null } as SessionSummary)
    await act(async () => useUiStore.setState({ sessionSummary: null }))
    expect(el.querySelector('[data-session-pill]')?.getAttribute('data-session-pill')).toBe('err')
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('Session summary unavailable')
  })
})

// owner-followups-1015 #11（C12a「改为头部胶囊」）：显隐、色调、播报与脉冲、键位在胶囊隐藏时仍生效、「稍后继续…」弹层。
describe('SessionPill（C12a 头部胶囊）', () => {
  let root: ReturnType<typeof createRoot> | undefined
  let el: HTMLDivElement
  const running = { ...base, session: { ...base.session!, status: 'running' as const, last_reason_code: null }, pending_count: 0, highest_pending_severity: null, next_action: null }
  const mount = async () => {
    await act(async () => {
      root = createRoot(el)
      root.render(<TooltipProvider><SessionPill /></TooltipProvider>)
    })
  }
  const pill = () => el.querySelector<HTMLButtonElement>('[data-session-pill]')
  const announce = () => el.querySelector('[data-session-announce]')!
  const setStatus = async (status: SessionSummary['session'] extends infer S ? S extends { status: infer T } ? T : never : never) => {
    await act(async () => useUiStore.setState({ sessionSummary: { ...running, session: { ...running.session!, status } } }))
  }
  beforeEach(() => {
    el = document.createElement('div')
    document.body.appendChild(el)
    useUiStore.setState({ modalScope: 'workbench', sessionError: null, pending: [], refreshFast: async () => {}, sessionLaterOpen: false })
  })
  afterEach(async () => {
    await act(async () => root?.unmount())
    el.remove()
    vi.restoreAllMocks()
    await i18n.changeLanguage('en')
  })

  it('stays out of the header while the session runs healthy, and appears with the worst tone otherwise', async () => {
    useUiStore.setState({ sessionSummary: running })
    await mount()
    expect(pill()).toBeNull()
    await setStatus('paused')
    expect(pill()?.getAttribute('data-session-pill')).toBe('info')
    expect(pill()?.textContent).toBe('Session · Paused')
    await act(async () => useUiStore.setState({ sessionSummary: { ...running, capabilities: [
      { kind: 'mcp', id: 'docs', available: true, granted: false, health: 'healthy', last_error: null, requires_user_action: false, binding_hint: 'p' },
      { kind: 'mcp', id: 'git', available: true, granted: false, health: 'unavailable', last_error: null, requires_user_action: true, binding_hint: 'p' },
    ] } }))
    expect(pill()?.getAttribute('data-session-pill')).toBe('err')
    expect(pill()?.textContent).toContain('1/2 healthy')
  })

  it('announces once and pulses once on entering failed / reconnecting, not when mounted already failed', async () => {
    useUiStore.setState({ sessionSummary: { ...running, session: { ...running.session!, status: 'failed' } } })
    await mount()
    // 挂载时已是 failed（如画布全宽切换换挂点重挂）不算「转入」：不重播、不脉冲
    expect(announce().textContent).toBe('')
    expect(pill()?.hasAttribute('data-session-pulse')).toBe(false)
    await setStatus('running')
    await setStatus('reconnecting')
    expect(announce().getAttribute('aria-live')).toBe('polite')
    expect(announce().textContent).toBe('Session reconnecting')
    expect(pill()?.className).toContain('animate-hx-pulse-once')
    // 单次：动画结束即撤类，不循环
    await act(async () => { pill()!.dispatchEvent(new Event('animationend', { bubbles: true })) })
    expect(pill()?.className).not.toContain('animate-hx-pulse-once')
    await setStatus('failed')
    expect(announce().textContent).toBe('Session failed')
    expect(pill()?.hasAttribute('data-session-pulse')).toBe(true)
  })

  it('keeps the pause shortcut working while the pill is hidden', async () => {
    const spy = vi.spyOn(api, 'sessionPause').mockResolvedValue(undefined)
    useUiStore.setState({ sessionSummary: running })
    await mount()
    expect(pill()).toBeNull()
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '9', code: 'Digit9', metaKey: isMac, ctrlKey: !isMac, altKey: true, shiftKey: true, bubbles: true }))
    })
    expect(spy).toHaveBeenCalled()
  })

  it('opens the continue-later picker from the store entry even while hidden, and closes it once a booking exists', async () => {
    useUiStore.setState({ sessionSummary: running })
    await mount()
    await act(async () => useUiStore.getState().setSessionLaterOpen(true))
    expect(document.querySelector('[data-session-later] [data-later-choice="in5"]')).not.toBeNull()
    await act(async () => useUiStore.setState({ sessionSummary: { ...running, triggers: [trigger('scheduled')] } }))
    expect(useUiStore.getState().sessionLaterOpen).toBe(false)
  })
})
