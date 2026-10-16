import { beforeEach, expect, it } from 'vitest'
import i18n from './i18n'
import type { TimelineItem } from './api'
import { delegationLabel } from './delegationLabel'
import { fmtTime } from './usage'

// 票 16B（managed#running）：托管系统行原先只有「托管设置已变更」；core 现在记 change 与保留的批准模式，
// 行文按原型写「时刻 · 你开启了托管 · 原模式「帮我批准」」。旧事件没有 change 时返回 null，回退通用文案。
const item = (payload: Record<string, unknown>) => ({
  event: { id: 1, kind: 'system', agent_id: null, stage_run_id: null, created_at: '2026-10-15 14:20:00', payload: { kind: 'delegation_changed', ...payload } },
}) as unknown as TimelineItem

beforeEach(async () => { await i18n.changeLanguage('zh-CN') })

it('names the transition, its time and the kept approval mode', () => {
  const time = fmtTime('2026-10-15 14:20:00')
  expect(delegationLabel(i18n.t, item({ change: 'on', approval_mode: 'assisted' }))).toBe(`${time} · 你开启了托管 · 原模式「帮我批准」`)
  expect(delegationLabel(i18n.t, item({ change: 'pause', approval_mode: 'assisted' }))).toBe(`${time} · 你暂停了托管`)
  expect(delegationLabel(i18n.t, item({ change: 'off' }))).toBe(`${time} · 你关闭了托管`)
})

it('falls back for events written before the change field existed', () => {
  expect(delegationLabel(i18n.t, item({ enabled: true, paused: false }))).toBeNull()
  expect(delegationLabel(i18n.t, item({ change: 'unchanged' }))).toBeNull()
})
