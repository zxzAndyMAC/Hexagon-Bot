import { beforeEach, describe, expect, it } from 'vitest'
import { useUiStore } from './store'
import type { TurnDelta } from './api'

const d = (over: Partial<TurnDelta>): TurnDelta => ({
  agent_id: 'a1',
  stage_run_id: null,
  call: 0,
  reset: false,
  done: false,
  text: '',
  ...over,
})

describe('applyDelta（turn-streaming 票 03）', () => {
  beforeEach(() => useUiStore.setState({ streams: {} }))

  it('文本增量按序累积', () => {
    const s = useUiStore.getState()
    s.applyDelta(d({ text: '你好' }))
    s.applyDelta(d({ text: '世界' }))
    expect(useUiStore.getState().streams.a1[0]).toBe('你好世界')
  })

  it('新一轮 call 分段，互不串扰', () => {
    const s = useUiStore.getState()
    s.applyDelta(d({ call: 0, text: '方案' }))
    s.applyDelta(d({ call: 1, text: '执行' }))
    expect(useUiStore.getState().streams.a1).toEqual({ 0: '方案', 1: '执行' })
  })

  it('reset 清掉本 call 已收文本（瞬时重试重吐全文）', () => {
    const s = useUiStore.getState()
    s.applyDelta(d({ text: '半截' }))
    s.applyDelta(d({ reset: true }))
    s.applyDelta(d({ text: '完整' }))
    expect(useUiStore.getState().streams.a1[0]).toBe('完整')
  })

  it('done 收走该 agent 的缓冲', () => {
    const s = useUiStore.getState()
    s.applyDelta(d({ text: 'x' }))
    s.applyDelta(d({ agent_id: 'a2', text: 'y' }))
    s.applyDelta(d({ done: true }))
    const streams = useUiStore.getState().streams
    expect(streams.a1).toBeUndefined()
    expect(streams.a2[0]).toBe('y')
  })
})
