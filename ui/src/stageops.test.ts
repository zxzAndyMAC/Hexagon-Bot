import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useUiStore } from './store'
import type { StageRow } from './gen/StageRow'

vi.mock('./api', async (importOriginal) => {
  const real = await importOriginal<typeof import('./api')>()
  return {
    ...real,
    api: {
      rewind: vi.fn().mockResolvedValue(undefined),
      skip: vi.fn().mockResolvedValue(undefined),
      stamp: vi.fn().mockResolvedValue(undefined),
      pause: vi.fn().mockResolvedValue(undefined),
      resume: vi.fn().mockResolvedValue(undefined),
      sleepAll: vi.fn().mockResolvedValue(undefined),
    },
  }
})

import { api } from './api'
import { runStageOp } from './stageops'

function stage(state: StageRow['state'], seq = 2): StageRow {
  return { run_id: 'r1', stage: 'build', seq, state }
}

beforeEach(() => {
  vi.clearAllMocks()
  useUiStore.setState({
    stages: [],
    confirmReq: null,
    toasts: [],
    modalScope: 'workbench',
  })
})

describe('runStageOp 确认分级（ADR 0056）', () => {
  it('rewind 不直执行，先挂 confirmReq（L2 流程破坏）', () => {
    useUiStore.setState({ stages: [stage('active')] })
    expect(runStageOp('rewind')).toBe(true)
    expect(api.rewind).not.toHaveBeenCalled()
    expect(useUiStore.getState().confirmReq?.danger).toBe(true)
  })

  it('confirmReq.run 真正调 api.rewind(active.seq - 1)', async () => {
    useUiStore.setState({ stages: [stage('active', 3)] })
    runStageOp('rewind')
    await useUiStore.getState().confirmReq!.run()
    expect(api.rewind).toHaveBeenCalledWith(2)
  })

  it('skip 同样过确认层', () => {
    useUiStore.setState({ stages: [stage('active')] })
    expect(runStageOp('skip')).toBe(true)
    expect(api.skip).not.toHaveBeenCalled()
    expect(useUiStore.getState().confirmReq?.danger).toBe(true)
  })

  it('pause/sleepAll 为 L1 可逆——直执行不弹确认', async () => {
    useUiStore.setState({ stages: [stage('active')] })
    expect(runStageOp('pause')).toBe(true)
    expect(runStageOp('sleepAll')).toBe(true)
    expect(useUiStore.getState().confirmReq).toBeNull()
    await vi.waitFor(() => {
      expect(api.pause).toHaveBeenCalled()
      expect(api.sleepAll).toHaveBeenCalled()
    })
  })

  it('stamp 仅在 waiting_stamp 态可用', async () => {
    useUiStore.setState({ stages: [stage('active')] })
    expect(runStageOp('stamp')).toBe(false)
    useUiStore.setState({ stages: [stage('waiting_stamp')] })
    expect(runStageOp('stamp')).toBe(true)
    await vi.waitFor(() => expect(api.stamp).toHaveBeenCalled())
  })

  it('无活动阶段时 rewind/skip/pause 返回 false 且无副作用', () => {
    useUiStore.setState({ stages: [stage('done')] })
    expect(runStageOp('rewind')).toBe(false)
    expect(runStageOp('skip')).toBe(false)
    expect(runStageOp('pause')).toBe(false)
    expect(useUiStore.getState().confirmReq).toBeNull()
  })

  it('seq=0 的 rewind 不可退（无前驱阶段）', () => {
    useUiStore.setState({ stages: [stage('active', 0)] })
    expect(runStageOp('rewind')).toBe(false)
  })

  it('IPC 失败走 toast 出口而不是静默/裸 throw', async () => {
    vi.mocked(api.pause).mockRejectedValueOnce(new Error('nope'))
    useUiStore.setState({ stages: [stage('active')] })
    runStageOp('pause')
    await vi.waitFor(() => {
      expect(useUiStore.getState().toasts.some((t) => t.tone === 'err')).toBe(true)
    })
  })
})
