import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useUiStore } from './store'
import { api, type TimelineItem, type TurnDelta } from './api'

describe('共享原语（ui-audit 票 01）', () => {
  beforeEach(() => useUiStore.setState({ toasts: [], modalScope: 'workbench' }))

  it('modalScope 切换与复位', () => {
    const s = useUiStore.getState()
    s.setModalScope('settings')
    expect(useUiStore.getState().modalScope).toBe('settings')
    s.setModalScope('workbench')
    expect(useUiStore.getState().modalScope).toBe('workbench')
  })

  it('pushToast 入栈并按超时自动消退', () => {
    vi.useFakeTimers()
    try {
      const s = useUiStore.getState()
      s.pushToast('失败一', 'err')
      s.pushToast('完成', 'ok')
      expect(useUiStore.getState().toasts).toHaveLength(2)
      expect(useUiStore.getState().toasts[0].tone).toBe('err')
      vi.advanceTimersByTime(3600)
      expect(useUiStore.getState().toasts).toHaveLength(0)
    } finally {
      vi.useRealTimers()
    }
  })

  it('dismissToast 点击提前关闭', () => {
    vi.useFakeTimers()
    try {
      useUiStore.getState().pushToast('x')
      const id = useUiStore.getState().toasts[0].id
      useUiStore.getState().dismissToast(id)
      expect(useUiStore.getState().toasts).toHaveLength(0)
    } finally {
      vi.useRealTimers()
    }
  })
})

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

  it('done 不清缓冲——标记完结等持久化交接（票 08）', () => {
    const s = useUiStore.getState()
    s.applyDelta(d({ text: 'x' }))
    s.applyDelta(d({ agent_id: 'a2', text: 'y' }))
    s.applyDelta(d({ done: true }))
    const st = useUiStore.getState()
    // 缓冲原地保留（气泡转落位样式），交接簿记下完结水位
    expect(st.streams.a1[0]).toBe('x')
    expect(st.streamDone.a1).toBeTruthy()
    expect(st.streamDone.a2).toBeUndefined()
  })
})

describe('流式交接（ui-audit 票 08 / P1-7）', () => {
  beforeEach(() =>
    useUiStore.setState({ streams: {}, streamDone: {}, timeline: [], stages: [], pending: [], toasts: [] }),
  )
  afterEach(() => vi.restoreAllMocks())

  const agentMsg = (id: number, author: string): TimelineItem => ({
    event: { id, project_id: 'p1', kind: 'agent_message', agent_id: author, stage_run_id: null, payload: {}, created_at: '' },
    message: { id, author, body: 'done text', tokens: [], attachments: [], created_at: '' },
  })

  it('持久消息到达后对应 stream 清除，其余保留', async () => {
    const s = useUiStore.getState()
    s.applyDelta(d({ agent_id: 'a1', text: 'hello' }))
    s.applyDelta(d({ agent_id: 'a1', done: true }))
    s.applyDelta(d({ agent_id: 'a2', text: 'wip' }))
    s.applyDelta(d({ agent_id: 'a2', done: true }))

    const lastId = useUiStore.getState().timeline.at(-1)?.event.id ?? 0
    vi.spyOn(api, 'timeline').mockResolvedValueOnce([agentMsg(lastId + 1, 'a1')])
    await useUiStore.getState().refreshFast()
    const st = useUiStore.getState()
    expect(st.streams.a1).toBeUndefined()   // 持久化确认 → 交接
    expect(st.streamDone.a1).toBeUndefined()
    expect(st.streams.a2[0]).toBe('wip')    // 别的 agent 不受影响
  })

  it('同 agent 旧水位之前的消息不触发交接', async () => {
    // 水位后没有该 agent 的新消息 → 不交接（dev mock 时间线含 a1
    // 历史消息，这里钉住：历史行不算交接凭证，必须 id > 水位）
    vi.spyOn(api, 'timeline').mockResolvedValue([])
    const s = useUiStore.getState()
    s.applyDelta(d({ agent_id: 'a1', text: 'hello' }))
    s.applyDelta(d({ agent_id: 'a1', done: true }))
    await useUiStore.getState().refreshFast()
    expect(useUiStore.getState().streams.a1[0]).toBe('hello')
    expect(useUiStore.getState().streamDone.a1).toBeTruthy()
  })

  it('超时兜底：done 超 STREAM_HANDOFF_MS 未持久化 → 清除防泄漏', async () => {
    vi.spyOn(api, 'timeline').mockResolvedValue([]) // 永不落库的路径
    vi.useFakeTimers()
    try {
      const s = useUiStore.getState()
      s.applyDelta(d({ agent_id: 'a1', text: 'hello' }))
      s.applyDelta(d({ agent_id: 'a1', done: true }))
      vi.advanceTimersByTime(11_000) // 越过兜底阈值（turn_failed 无消息路径）
      await useUiStore.getState().refreshFast()
      const st = useUiStore.getState()
      expect(st.streams.a1).toBeUndefined()
      expect(st.streamDone.a1).toBeUndefined()
    } finally {
      vi.useRealTimers()
    }
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

describe('错误出口（ui-audit 票 04 / P1-6）', () => {
  beforeEach(() =>
    useUiStore.setState({ toasts: [], timeline: [], stages: [], pending: [], avatarHashes: {}, avatars: {} }),
  )
  afterEach(() => vi.restoreAllMocks())

  it('refreshSlow 单切片失败不拖垮其余切片 + 一次提示', async () => {
    vi.spyOn(api, 'usage').mockRejectedValueOnce(new Error('usage down'))
    await useUiStore.getState().refreshSlow()
    const s = useUiStore.getState()
    expect(s.artifacts.length).toBeGreaterThan(0) // 其余切片照常落地
    expect(s.team.length).toBeGreaterThan(0)
    expect(s.toasts.filter((t) => t.tone === 'err')).toHaveLength(1) // 一次聚合提示
  })

  it('projectInfo 失败不再把 mode/packName 重置回默认（旧 bug 回归）', async () => {
    useUiStore.setState({ mode: 'fastpath', packName: 'X', fastRole: 'r' })
    vi.spyOn(api, 'projectInfo').mockRejectedValueOnce(new Error('info down'))
    await useUiStore.getState().refreshSlow(['info'])
    const s = useUiStore.getState()
    expect(s.mode).toBe('fastpath')
    expect(s.packName).toBe('X')
    expect(s.toasts.some((t) => t.tone === 'err')).toBe(true)
  })

  it('refreshFast 连续失败第 3 拍才 toast 一次，恢复后复位', async () => {
    const spy = vi.spyOn(api, 'stageStatus').mockRejectedValue(new Error('net down'))
    const errToasts = () => useUiStore.getState().toasts.filter((t) => t.tone === 'err').length
    await useUiStore.getState().refreshFast()
    await useUiStore.getState().refreshFast()
    expect(errToasts()).toBe(0) // 前两次静默——轮询抖动不打扰
    await useUiStore.getState().refreshFast()
    expect(errToasts()).toBe(1) // 第三次连续失败提示一次
    await useUiStore.getState().refreshFast()
    expect(errToasts()).toBe(1) // 不刷屏

    spy.mockResolvedValue([] as never)
    await useUiStore.getState().refreshFast() // 恢复 → 计数复位
    spy.mockRejectedValue(new Error('net down'))
    await useUiStore.getState().refreshFast()
    await useUiStore.getState().refreshFast()
    expect(errToasts()).toBe(1) // 重新计满 3 才再提示
    await useUiStore.getState().refreshFast()
    expect(errToasts()).toBe(2)
  })
})

describe('代际守卫（ui-audit 票 06 / P2-9）', () => {
  beforeEach(() => useUiStore.setState({ toasts: [], timeline: [], stages: [], pending: [] }))
  afterEach(() => vi.restoreAllMocks())

  it('refreshFast 在飞期间调 refresh()：旧响应落地被丢弃', async () => {
    // 旧项目的慢响应：resolve 时携带一条幽灵事件
    let resolveStale!: (v: TimelineItem[]) => void
    const stalePromise = new Promise<TimelineItem[]>((r) => { resolveStale = r })
    const spy = vi.spyOn(api, 'timeline')
      .mockImplementationOnce(() => stalePromise)

    const staleCall = useUiStore.getState().refreshFast() // 起飞，捕获旧代际
    // 切项目：refresh() 递增代际 + 清空游标 + 正常拉新一轮
    await useUiStore.getState().refresh()
    const baseline = useUiStore.getState().timeline.length

    resolveStale([mkItem(99999)]) // 旧响应此刻才回来——必须作废
    await staleCall
    const ids = useUiStore.getState().timeline.map((i) => i.event.id)
    expect(ids).not.toContain(99999)
    expect(useUiStore.getState().timeline.length).toBe(baseline)
    spy.mockRestore()
  })

  it('refreshSlow 在飞期间 refresh()：旧切片不覆盖新状态', async () => {
    let resolveStale!: (v: unknown) => void
    const stalePromise = new Promise((r) => { resolveStale = r })
    vi.spyOn(api, 'team').mockImplementationOnce(() => stalePromise as Promise<never>)
    useUiStore.setState({ team: [] })

    const staleCall = useUiStore.getState().refreshSlow(['team'])
    await useUiStore.getState().refresh() // 代际前移
    const newTeamLen = useUiStore.getState().team.length

    resolveStale([{ id: 'ghost', project_id: 'p0', role: '旧角色', status: 'active', avatar_hash: null, model_slot: null }])
    await staleCall
    expect(useUiStore.getState().team.some((m) => m.id === 'ghost')).toBe(false)
    expect(useUiStore.getState().team.length).toBe(newTeamLen)
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

describe('值守循环（ui-audit 票 16 / 方向卡 1）', () => {
  beforeEach(() =>
    useUiStore.setState({ away: false, toasts: [], timeline: [], stages: [], pending: [] }),
  )
  afterEach(() => vi.restoreAllMocks())

  it('markAway 调 owner_away 并置位；重复调用幂等', async () => {
    const spy = vi.spyOn(api, 'ownerAway').mockResolvedValue(undefined)
    await useUiStore.getState().markAway()
    expect(spy).toHaveBeenCalledTimes(1)
    expect(useUiStore.getState().away).toBe(true)
    await useUiStore.getState().markAway()
    expect(spy).toHaveBeenCalledTimes(1)
  })

  it('markBack 调 owner_back、复位并 refreshFast 拉摘要行；失败走 toast', async () => {
    useUiStore.setState({ away: true })
    const back = vi.spyOn(api, 'ownerBack').mockResolvedValue({} as Awaited<ReturnType<typeof api.ownerBack>>)
    const fast = vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
    await useUiStore.getState().markBack()
    expect(back).toHaveBeenCalledTimes(1)
    expect(fast).toHaveBeenCalledTimes(1)
    expect(useUiStore.getState().away).toBe(false)

    useUiStore.setState({ away: true })
    back.mockRejectedValue(new Error('io down'))
    await useUiStore.getState().markBack()
    expect(useUiStore.getState().away).toBe(true) // 失败不复位
    expect(useUiStore.getState().toasts.length).toBe(1)
  })
})

describe('夜航主题（票 21 / 方向卡 6）', () => {
  it('setThemePref("night") 即时生效且持久化；重启经 localStorage 保持', () => {
    useUiStore.getState().setThemePref('night')
    expect(document.documentElement.dataset.theme).toBe('night')
    expect(localStorage.getItem('hexagon.theme')).toBe('night')
    useUiStore.getState().setThemePref('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')
  })
})
