import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useUiStore } from './store'
import { api, type TimelineItem, type TurnDelta } from './api'

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

// 增量通道（arch-review 票 07）：timeline 走 after 游标追加，
// refresh() 是唯一的全量重置点。
const mkItem = (id: number): TimelineItem => ({
  event: {
    id, project_id: 'p1', kind: 'system', agent_id: null,
    stage_run_id: null, payload: {}, created_at: `2026-09-18T12:${String(id).padStart(2, '0')}:00Z`,
  },
  message: null,
})

describe('refreshFast 增量归并（arch-review 票 07）', () => {
  beforeEach(() => useUiStore.setState({ timeline: [], stages: [], pending: [] }))
  afterEach(() => vi.restoreAllMocks())

  it('空时间线首拉全量；之后按末条 id 增量', async () => {
    const spy = vi.spyOn(api, 'timeline')
    await useUiStore.getState().refreshFast()
    expect(spy).toHaveBeenCalledWith(undefined)
    const n = useUiStore.getState().timeline.length
    expect(n).toBeGreaterThan(0)
    const lastId = useUiStore.getState().timeline.at(-1)!.event.id

    await useUiStore.getState().refreshFast()
    expect(spy).toHaveBeenLastCalledWith(lastId)
    expect(useUiStore.getState().timeline.length).toBe(n) // mock 无新事件 → 不增
  })

  it('新事件追加；重复返回同批幂等', async () => {
    const spy = vi.spyOn(api, 'timeline')
    await useUiStore.getState().refreshFast()
    const n = useUiStore.getState().timeline.length
    const lastId = useUiStore.getState().timeline.at(-1)!.event.id

    spy.mockResolvedValueOnce([mkItem(lastId + 1)])
    await useUiStore.getState().refreshFast()
    expect(useUiStore.getState().timeline.at(-1)!.event.id).toBe(lastId + 1)
    expect(useUiStore.getState().timeline.length).toBe(n + 1)

    // 实现漂移返回重复段：客户端 id 闸挡住
    spy.mockResolvedValueOnce([mkItem(lastId + 1), mkItem(lastId + 2)])
    await useUiStore.getState().refreshFast()
    const ids = useUiStore.getState().timeline.map((i) => i.event.id)
    expect(new Set(ids).size).toBe(ids.length)
    expect(useUiStore.getState().timeline.at(-1)!.event.id).toBe(lastId + 2)
  })

  it('refresh() 全量重置游标', async () => {
    useUiStore.setState({ timeline: [mkItem(9999)] })
    await useUiStore.getState().refresh()
    const tl = useUiStore.getState().timeline
    expect(tl[0].event.id).toBe(1) // 回到 mock 首段，假数据被清
    expect(tl.some((i) => i.event.id === 9999)).toBe(false)
  })
})

describe('invalidate 失效标签（arch-review 票 07）', () => {
  beforeEach(() => useUiStore.setState({ timeline: [], stages: [], pending: [], avatarHashes: {}, avatars: {} }))
  afterEach(() => vi.restoreAllMocks())

  it('refreshFast 稳态只打 3 个端点', async () => {
    const spies = {
      stageStatus: vi.spyOn(api, 'stageStatus'),
      pendingQuestions: vi.spyOn(api, 'pendingQuestions'),
      timeline: vi.spyOn(api, 'timeline'),
      usage: vi.spyOn(api, 'usage'),
      artifacts: vi.spyOn(api, 'artifacts'),
      team: vi.spyOn(api, 'team'),
      autonomy: vi.spyOn(api, 'autonomy'),
      projectInfo: vi.spyOn(api, 'projectInfo'),
    }
    await useUiStore.getState().refreshFast()
    expect(spies.stageStatus).toHaveBeenCalled()
    expect(spies.pendingQuestions).toHaveBeenCalled()
    expect(spies.timeline).toHaveBeenCalled()
    for (const k of ['usage', 'artifacts', 'team', 'autonomy', 'projectInfo'] as const) {
      expect(spies[k], k).not.toHaveBeenCalled()
    }
  })

  it('invalidate() 无参 = 全切片；带标签只拉对应域', async () => {
    const spies = {
      usage: vi.spyOn(api, 'usage'),
      artifacts: vi.spyOn(api, 'artifacts'),
      team: vi.spyOn(api, 'team'),
      autonomy: vi.spyOn(api, 'autonomy'),
      projectInfo: vi.spyOn(api, 'projectInfo'),
    }
    await useUiStore.getState().invalidate()
    for (const spy of Object.values(spies)) expect(spy).toHaveBeenCalled()

    vi.clearAllMocks()
    await useUiStore.getState().invalidate('usage')
    expect(spies.usage).toHaveBeenCalled()
    for (const k of ['artifacts', 'team', 'autonomy', 'projectInfo'] as const) {
      expect(spies[k], k).not.toHaveBeenCalled()
    }
  })

  it('invalidate 同时补快通道一拍（写后 pending/stages 立即一致）', async () => {
    const spies = {
      stageStatus: vi.spyOn(api, 'stageStatus'),
      pendingQuestions: vi.spyOn(api, 'pendingQuestions'),
      timeline: vi.spyOn(api, 'timeline'),
    }
    await useUiStore.getState().invalidate('usage')
    for (const spy of Object.values(spies)) expect(spy).toHaveBeenCalled()
  })
})

describe('avatar 哈希条件拉取（arch-review 票 07）', () => {
  beforeEach(() => useUiStore.setState({ avatarHashes: {}, avatars: {} }))
  afterEach(() => vi.restoreAllMocks())

  it('哈希不变不拉；变了才拉；拉失败记 null 下轮重试', async () => {
    const spy = vi.spyOn(api, 'agentAvatar')
    await useUiStore.getState().invalidate('team')
    // mock 只有 a1 有头像 → 恰好 1 次拉取
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy).toHaveBeenCalledWith('a1')
    expect(useUiStore.getState().avatars.a1).toBeTruthy()

    spy.mockClear()
    await useUiStore.getState().invalidate('team')
    expect(spy).not.toHaveBeenCalled() // 哈希未变 → 零拉取

    // 给 a2 设头像 → team 行哈希变化 → 只拉 a2
    await api.setAgentAvatar('a2', 'data:image/png;base64,AAAA')
    await useUiStore.getState().invalidate('team')
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy).toHaveBeenCalledWith('a2')
    expect(useUiStore.getState().avatars.a2).toBe('data:image/png;base64,AAAA')
  })
})
