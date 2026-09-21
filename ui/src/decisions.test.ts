// ui-audit 票 02：键盘裁决守卫——作用域 / publish 禁键 / 双发防护。
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { handlePendingKey, rejectReasonWithJudge, severityOf } from './decisions'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'

const card = (over: Partial<PendingQuestion>): PendingQuestion => ({
  id: 'q1',
  kind: 'permission',
  agent_id: null,
  payload: {},
  state: 'queued',
  ...over,
})

// 票 15：mod 平台精确化后测试环境（happy-dom，非 mac）的 mod = ctrlKey。
const key = (k: string, mods: { meta?: boolean; ctrl?: boolean; repeat?: boolean } = {}) =>
  new KeyboardEvent('keydown', {
    key: k,
    metaKey: mods.meta ?? false,
    ctrlKey: mods.ctrl ?? true,
    repeat: mods.repeat ?? false,
  })

describe('handlePendingKey（ui-audit 票 02）', () => {
  beforeEach(() => {
    useUiStore.setState({ modalScope: 'workbench', toasts: [] })
    vi.restoreAllMocks()
  })

  it('非裁决键静默放行', async () => {
    const spy = vi.spyOn(api, 'answerPermission')
    expect(await handlePendingKey(key('x'), [card({})])).toBe('none')
    expect(spy).not.toHaveBeenCalled()
  })

  it('无待决时不触发', async () => {
    const spy = vi.spyOn(api, 'answerPermission')
    expect(await handlePendingKey(key('Enter'), [])).toBe('none')
    expect(spy).not.toHaveBeenCalled()
  })

  it('P0-1：设置页作用域下 ⌘↵ 不裁决，出提示', async () => {
    useUiStore.setState({ modalScope: 'settings' })
    const spy = vi.spyOn(api, 'answerPermission')
    expect(await handlePendingKey(key('Enter'), [card({})])).toBe('blocked-scope')
    expect(spy).not.toHaveBeenCalled()
    expect(useUiStore.getState().toasts.length).toBe(1)
  })

  it('P0-1：palette 作用域同样拦截', async () => {
    useUiStore.setState({ modalScope: 'palette' })
    const spy = vi.spyOn(api, 'answerPermission')
    expect(await handlePendingKey(key('Enter'), [card({})])).toBe('blocked-scope')
    expect(spy).not.toHaveBeenCalled()
  })

  it('P0-2：顶卡为 publish 时批准键无效且提示', async () => {
    const spy = vi.spyOn(api, 'confirmPublish')
    const out = await handlePendingKey(key('Enter'), [card({ kind: 'publish' })])
    expect(out).toBe('blocked-publish')
    expect(spy).not.toHaveBeenCalled()
    expect(useUiStore.getState().toasts.length).toBe(1)
  })

  it('P0-2：publish 卡的驳回键仍可用', async () => {
    const spy = vi.spyOn(api, 'rejectPublish')
    const out = await handlePendingKey(key('Backspace'), [card({ kind: 'publish' })])
    expect(out).toBe('rejected')
    expect(spy).toHaveBeenCalledWith('q1')
  })

  it('正常批准：顶卡 permission → answerPermission(true)', async () => {
    const spy = vi.spyOn(api, 'answerPermission')
    expect(await handlePendingKey(key('Enter'), [card({})])).toBe('approved')
    expect(spy).toHaveBeenCalledWith('q1', true)
  })

  it('双发防护：repeat 事件与并发调用都只落一次', async () => {
    const spy = vi.spyOn(api, 'answerPermission')
    expect(await handlePendingKey(key('Enter', { repeat: true }), [card({})])).toBe('repeat')
    expect(spy).not.toHaveBeenCalled()

    // 并发：第一次未完成时第二次被拦
    let release!: () => void
    spy.mockImplementationOnce(() => new Promise<void>((r) => { release = r }))
    const p1 = handlePendingKey(key('Enter'), [card({})])
    const p2 = handlePendingKey(key('Enter'), [card({})])
    release()
    expect(await p1).toBe('approved')
    expect(await p2).toBe('repeat')
    expect(spy).toHaveBeenCalledTimes(1)
  })

  it('严重度序：recovery/publish/install 先于 permission', () => {
    const a = card({ id: 'a', kind: 'permission' })
    const b = card({ id: 'b', kind: 'publish' })
    expect(severityOf(b)).toBeLessThan(severityOf(a))
  })
})

describe('逆建议留痕（ui-audit 票 07 / P2-15）', () => {
  it('judge=stamp 时驳回 reason 追加 judge=stamp 对照', () => {
    expect(rejectReasonWithJudge('', 'stamp')).toBe('owner rejected · judge=stamp')
    expect(rejectReasonWithJudge('  方案不稳 ', 'stamp')).toBe('方案不稳 · judge=stamp')
  })

  it('judge=reject/needs_human 或无建议时 reason 原样（默认回填）', () => {
    expect(rejectReasonWithJudge('', 'reject')).toBe('owner rejected')
    expect(rejectReasonWithJudge('', 'needs_human')).toBe('owner rejected')
    expect(rejectReasonWithJudge('', null)).toBe('owner rejected')
    expect(rejectReasonWithJudge('有理由', 'reject')).toBe('有理由')
  })
})
