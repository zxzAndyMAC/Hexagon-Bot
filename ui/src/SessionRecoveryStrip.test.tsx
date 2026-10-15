import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { SessionRecoveryStrip, defaultWorktreePath } from './components/SessionRecoveryStrip'
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

describe('SessionHost recovery strip (C12a/C12b)', () => {
  let root: ReturnType<typeof createRoot> | undefined
  let el: HTMLDivElement
  const render = async (summary: SessionSummary) => {
    useUiStore.setState({ sessionSummary: summary })
    await act(async () => {
      root = createRoot(el)
      root.render(<TooltipProvider><SessionRecoveryStrip /></TooltipProvider>)
    })
  }
  const buttons = () => [...el.querySelectorAll('button')].map((b) => b.textContent ?? '')
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

  it('renders status, turn, pending tier and the resume action in a non-English locale', async () => {
    await i18n.changeLanguage('zh-CN')
    await render(base)
    const text = el.querySelector('[data-session-recovery]')?.textContent ?? ''
    expect(text).toContain('已暂停')
    expect(text).toContain('回合 run-1')
    expect(text).toContain('待决 1 · 最高档')
    expect(text).toContain('owner_paused')
    expect(buttons().some((b) => b.includes('继续会话'))).toBe(true)
    expect(text).not.toContain('session-1a2b3c4d5e')
  })

  it('keeps waiting_user in the pending-card path and shows an expired trigger with Continue', async () => {
    await render({ ...base, session: { ...base.session!, status: 'waiting_user' }, recovery_run_id: null, triggers: [trigger('expired')], next_action: 'review_pending' })
    expect(el.querySelector('[data-session-trigger="expired"]')?.textContent).toContain('Expired')
    expect(buttons().some((b) => b.includes('Resume session'))).toBe(false)
    expect(buttons().some((b) => b === 'Continue')).toBe(true)
    expect(el.textContent).toContain('Review pending decisions')
  })

  it('does not offer a second reconnect while the host is already reconnecting', async () => {
    await render({ ...base, session: { ...base.session!, status: 'reconnecting' }, next_action: 'wait_runtime' })
    expect(buttons().some((b) => b.includes('Reconnect'))).toBe(false)
    expect(el.textContent).toContain('Wait for runtime')
  })

  it('does not offer reconnect once the budget is exhausted; the next step is a new task', async () => {
    const spy = vi.spyOn(api, 'sessionReconnect').mockResolvedValue(undefined)
    await render({ ...base, session: { ...base.session!, status: 'failed', last_reason_code: 'reconnect_budget_exhausted' }, next_action: 'start_new_task' })
    expect(el.textContent).toContain('reconnect_budget_exhausted')
    expect(el.textContent).toContain('Send a new instruction to start a new task')
    expect(buttons().some((b) => b.includes('Reconnect'))).toBe(false)
    expect(spy).not.toHaveBeenCalled()
  })

  it('offers reconnect after a failure that still has budget, and calls core', async () => {
    const spy = vi.spyOn(api, 'sessionReconnect').mockResolvedValue(undefined)
    await render({ ...base, session: { ...base.session!, status: 'failed', last_reason_code: 'turn_failed' }, next_action: 'inspect_failure' })
    expect(el.textContent).toContain('turn_failed')
    const reconnect = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Reconnect session')!
    await act(async () => { reconnect.click() })
    expect(spy).toHaveBeenCalled()
  })

  it('shows health and proven authorization separately', async () => {
    await render({ ...base, capabilities: [
      { kind: 'mcp', id: 'docs', available: true, granted: false, health: 'unavailable', last_error: 'mcp_worker_exited', requires_user_action: true, binding_hint: 'p' },
      { kind: 'browser', id: 'browser', available: true, granted: true, health: 'healthy', last_error: null, requires_user_action: false, binding_hint: 'p' },
    ] })
    const caps = el.querySelector('[data-session-capabilities]')!
    const badges = [...caps.querySelectorAll('[data-slot="badge"]')]
    expect(badges[0].getAttribute('data-variant')).toBe('err')
    expect(badges[0].querySelector('[aria-label="Authorization proven"]')).toBeNull()
    expect(badges[1].getAttribute('data-variant')).toBe('ok')
    expect(badges[1].querySelector('[aria-label="Authorization proven"]')).not.toBeNull()
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
    const cleanup = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Clean up worktree')!
    await act(async () => { cleanup.click() })
    expect(spy).toHaveBeenCalledWith('cleanup')
    await act(async () => root?.unmount())
    useUiStore.setState({ pending: [{ id: 'q9', kind: 'worktree', agent_id: null, payload: { operation: 'cleanup' }, state: 'queued' }] })
    await render(withTree)
    expect(buttons().includes('Clean up worktree')).toBe(false)
    expect(el.querySelector('[data-session-worktree]')?.textContent).toContain('Awaiting confirmation')
  })

  it('offers only a read-only verify while a worktree outcome is unknown', async () => {
    const spy = vi.spyOn(api, 'sessionVerifyWorktree').mockResolvedValue(base.session!)
    await render({ ...base, session: { ...base.session!, worktree_intent: { operation: 'create', question_id: 'q1', path: '/work/demo-worktrees/x', baseline_commit: 'abc', state: 'executing', reason_code: null } }, next_action: 'verify_worktree' })
    expect(el.textContent).toContain('Outcome unknown')
    expect(buttons().some((b) => b.includes('Create worktree'))).toBe(false)
    const verify = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Verify result')!
    await act(async () => { verify.click() })
    expect(spy).toHaveBeenCalled()
  })

  // 票 13（SessionHost 影响审查 2026-10-13）：工作树动作失败走失败 toast，事实漂移写原因码（D8）。
  it('surfaces a worktree drift failure as an error toast carrying worktree_binding_drift', async () => {
    useUiStore.setState({ toasts: [] })
    vi.spyOn(api, 'sessionVerifyWorktree').mockRejectedValue({ code: 'worktree_binding_drift', message: 'worktree HEAD moved' })
    await render({ ...base, session: { ...base.session!, worktree_intent: { operation: 'create', question_id: 'q1', path: '/work/demo-worktrees/x', baseline_commit: 'abc', state: 'executing', reason_code: null } }, next_action: 'verify_worktree' })
    const verify = [...el.querySelectorAll('button')].find((b) => b.textContent === 'Verify result')!
    await act(async () => { verify.click() })
    expect(useUiStore.getState().toasts.at(-1)).toMatchObject({ tone: 'err', code: 'worktree_binding_drift' })
  })

  it('derives the default worktree path outside the project root', () => {
    expect(defaultWorktreePath({ project_root: '/work/demo/', session_id: 'session-1a2b3c4d5e' })).toBe('/work/demo-worktrees/1a2b3c4d')
  })

  it('shows a retryable error card when the summary cannot be read', async () => {
    useUiStore.setState({ sessionError: 'boom' })
    await render({ ...base, session: null } as SessionSummary)
    useUiStore.setState({ sessionSummary: null })
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('Session summary unavailable')
  })
})
