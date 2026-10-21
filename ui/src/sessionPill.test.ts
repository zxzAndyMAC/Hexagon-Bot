// 会话胶囊显示条件表驱动测试（components C12a「改为头部胶囊」，owner-followups-1015 #11）。
import { describe, expect, it } from 'vitest'
import { alertTransition, groupCapabilities, laterBlockedReason, sessionPillView } from './sessionPill'
import type { CapabilityStatus } from './gen/CapabilityStatus'
import type { OneTimeTrigger } from './gen/OneTimeTrigger'
import type { SessionStatus } from './gen/SessionStatus'
import type { SessionSummary } from './gen/SessionSummary'

const cap = (kind: CapabilityStatus['kind'], id: string, health: CapabilityStatus['health']): CapabilityStatus => ({
  kind, id, available: true, granted: false, health, last_error: null, requires_user_action: false, binding_hint: 'p',
})
const trig = (state: OneTimeTrigger['state']): OneTimeTrigger => ({
  trigger_id: `t-${state}`, session_id: 's', project_root: '/p', baseline: 'main', model_slot: 'default',
  permission_package: [], fire_at: 1, trace_id: 'x', state, created_at: '', consumed_at: null,
})
const summary = (status: SessionStatus, capabilities: CapabilityStatus[] = [], triggers: OneTimeTrigger[] = []): SessionSummary => ({
  session: {
    session_id: 's', project_id: 'p', project_root: '/p', baseline: 'main', status, current_turn_id: null, worktree: null,
    worktree_intent: null, runtime: null, reconnect_attempts: 0, reconnect_budget: 3, is_current: true, created_at: '',
    updated_at: '', last_branch: null, last_reason_code: null, last_elapsed_ms: 0,
  },
  recovery_run_id: null, pending_count: 0, highest_pending_severity: null, recent_diagnostics: [], capabilities, triggers, next_action: null,
})

describe('sessionPillView（C12a 出现条件）', () => {
  const rows: [string, SessionSummary | null, string | null, string][] = [
    ['无会话无错误', null, null, 'hidden'],
    ['摘要读取失败', null, 'boom', 'err:error'],
    ['运行中 + 全健康 + 无预约', summary('running', [cap('mcp', 'a', 'healthy'), cap('browser', 'b', 'healthy')]), null, 'hidden'],
    ['created 等同运行中', summary('created'), null, 'hidden'],
    ['waiting_user 由坞承担，不出胶囊', summary('waiting_user'), null, 'hidden'],
    ['unknown 健康不算异常', summary('running', [cap('provider', 'default', 'unknown')]), null, 'hidden'],
    ['已取消 / 已消费的预约不算', summary('running', [], [trig('cancelled'), trig('consumed')]), null, 'hidden'],
    ['暂停', summary('paused'), null, 'info:status'],
    ['重连中', summary('reconnecting'), null, 'warn:status'],
    ['失败', summary('failed'), null, 'err:status'],
    ['已完成', summary('completed'), null, 'ok:status'],
    ['能力降级', summary('running', [cap('mcp', 'a', 'degraded')]), null, 'warn:capability'],
    ['能力重连中', summary('running', [cap('browser', 'b', 'reconnecting')]), null, 'warn:capability'],
    ['能力不可用', summary('running', [cap('mcp', 'a', 'healthy'), cap('mcp', 'b', 'unavailable')]), null, 'err:capability'],
    ['有预约', summary('running', [], [trig('scheduled')]), null, 'neutral:scheduled'],
    ['预约过期', summary('running', [], [trig('expired')]), null, 'warn:expired'],
    ['色随最严重项：暂停 + 能力不可用 → err', summary('paused', [cap('mcp', 'a', 'unavailable')]), null, 'err:capability'],
    ['同档先状态：失败 + 能力不可用', summary('failed', [cap('mcp', 'a', 'unavailable')]), null, 'err:status'],
    ['同档先能力：能力降级 + 预约过期', summary('running', [cap('mcp', 'a', 'degraded')], [trig('expired')]), null, 'warn:capability'],
    ['已完成 + 有预约 → ok 压过 neutral', summary('completed', [], [trig('scheduled')]), null, 'ok:status'],
    ['有会话时读取错误不顶替事实', summary('paused'), 'boom', 'info:status'],
  ]
  it.each(rows)('%s', (_name, input, error, expected) => {
    const view = sessionPillView(input, error)
    expect(view.kind === 'hidden' ? 'hidden' : `${view.tone}:${view.reason.kind}`).toBe(expected)
  })
})

describe('groupCapabilities（能力按 kind 合并）', () => {
  it('数健康项、色取最差、保留 kind 首次出现次序', () => {
    const groups = groupCapabilities([
      cap('mcp', 'docs', 'healthy'), cap('browser', 'browser', 'healthy'), cap('mcp', 'git', 'unavailable'),
      cap('mcp', 'fs', 'degraded'), cap('mcp', 'web', 'healthy'), cap('mcp', 'db', 'unknown'), cap('mcp', 'x', 'healthy'),
    ])
    expect(groups.map((g) => [g.kind, g.healthy, g.total, g.worst])).toEqual([
      ['mcp', 3, 6, 'unavailable'],
      ['browser', 1, 1, 'healthy'],
    ])
    expect(groups[0].items.map((c) => c.id)).toEqual(['docs', 'git', 'fs', 'web', 'db', 'x'])
  })
  it('unknown 比 healthy 差、比 degraded 好', () => {
    expect(groupCapabilities([cap('provider', 'a', 'healthy'), cap('provider', 'b', 'unknown')])[0].worst).toBe('unknown')
    expect(groupCapabilities([cap('provider', 'a', 'unknown'), cap('provider', 'b', 'degraded')])[0].worst).toBe('degraded')
  })
})

describe('alertTransition（转入 failed / reconnecting 播报一次）', () => {
  const rows: [SessionStatus | null | undefined, SessionStatus | null, string | null][] = [
    [undefined, 'failed', null],
    ['running', 'failed', 'failed'],
    ['running', 'reconnecting', 'reconnecting'],
    ['reconnecting', 'failed', 'failed'],
    ['failed', 'failed', null],
    ['failed', 'reconnecting', 'reconnecting'],
    ['reconnecting', 'running', null],
    [null, 'failed', 'failed'],
    ['paused', 'completed', null],
  ]
  it.each(rows)('%s → %s', (prev, next, expected) => {
    expect(alertTransition(prev, next)).toBe(expected)
  })
})

describe('laterBlockedReason（「稍后继续…」可用性）', () => {
  const rows: [string, SessionSummary | null, string | null][] = [
    ['无会话', null, 'session.laterNoSession'],
    ['运行中', summary('running'), null],
    ['暂停可约', summary('paused'), null],
    ['已完成', summary('completed'), 'session.laterEnded'],
    ['已失败', summary('failed'), 'session.laterEnded'],
    ['已有预约', summary('running', [], [trig('scheduled')]), 'session.laterBooked'],
    ['只有过期预约仍可约', summary('running', [], [trig('expired')]), null],
  ]
  it.each(rows)('%s', (_name, input, expected) => {
    expect(laterBlockedReason(input)).toBe(expected)
  })
})
