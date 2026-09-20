import { create } from 'zustand'
import {
  api,
  type ArtifactRow,
  type PendingQuestion,
  type StageRow,
  type TeamRow,
  type TimelineItem,
  type TurnDelta,
  type UsageRow,
} from './api'

export type ThemePref = 'light' | 'dark' | 'system'

/// 慢通道切片（arch-review 票 07）：invalidate 的失效标签。
/// usage=账本；artifacts=产物表；team=花名册+头像；info=项目元信息+autonomy。
export type SlowSlice = 'usage' | 'artifacts' | 'team' | 'info'

// 中栏选项卡（ADR 0051）：timeline 固定主 tab，其余可关。
export type TabKind = 'timeline' | 'artifact' | 'diff' | 'agent' | 'usage'

export interface WorkTab {
  id: string // timeline | art:<path> | diff:<path>:<a>-<b> | agent:<id> | patch:<proposalId>
  kind: TabKind
  title: string
  path?: string
  version?: number
  agentId?: string
  role?: string
  diffFrom?: number
  diffTo?: number
  patchText?: string
}

const TIMELINE_TAB: WorkTab = { id: 'timeline', kind: 'timeline', title: '' } // title 渲染时走 i18n

function resolve(pref: ThemePref): 'light' | 'dark' {
  if (pref !== 'system') return pref
  return matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

const savedPref = (localStorage.getItem('hexagon.theme') as ThemePref) || 'dark'
document.documentElement.dataset.theme = resolve(savedPref)
// system 档跟随 OS 切换
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
  const s = useUiStore.getState()
  if (s.themePref === 'system') document.documentElement.dataset.theme = resolve('system')
})

interface UiState {
  themePref: ThemePref
  stages: StageRow[]
  team: TeamRow[]
  artifacts: ArtifactRow[]
  timeline: TimelineItem[]
  pending: PendingQuestion[]
  usageTotal: { spent_mc: number; limit_cents: number | null; tokens: number } | null
  autonomy: string
  projectName: string
  mode: 'pack' | 'fastpath'
  fastRole: string | null
  packName: string | null
  railOpen: boolean
  sideTab: 'artifacts' | 'team' | 'usage'
  usageRows: UsageRow[]
  avatars: Record<string, string>
  tabs: WorkTab[]
  activeTab: string
  splitOpen: boolean
  /// 流式增量缓冲（票 03）：agent → 调用序号 → 累计文本。
  /// 瞬时态——不落盘；done 信号到达即清对应 agent。
  streams: Record<string, Record<number, string>>
  setThemePref: (p: ThemePref) => void
  setRailOpen: (v: boolean) => void
  setSideTab: (t: 'artifacts' | 'team' | 'usage') => void
  openTab: (t: WorkTab) => void
  closeTab: (id: string) => void
  setActiveTab: (id: string) => void
  setSplitOpen: (v: boolean) => void
  applyDelta: (d: TurnDelta) => void
  refresh: () => Promise<void>
  /// 快通道（arch-review 票 07）：2s 轮询面 = stages+pending+timeline 增量
  /// （3 invoke 稳态）。timeline 走 after 游标追加；空时间线=首拉全量。
  refreshFast: () => Promise<void>
  /// 慢通道：usage/artifacts/team(+avatars)/info，仅失效标签驱动，不轮询。
  /// tags 缺省 = 全切片。
  refreshSlow: (tags?: SlowSlice[]) => Promise<void>
  /// 组件写操作后统一入口（取代散点 await refresh()）：
  /// 快通道补一拍 + 按标签拉慢切片。`invalidate()` = 全失效。
  invalidate: (...tags: SlowSlice[]) => Promise<void>
}

export const useUiStore = create<UiState>((set) => ({
  themePref: savedPref,
  stages: [],
  team: [],
  artifacts: [],
  timeline: [],
  pending: [],
  usageTotal: null,
  autonomy: 'L0',
  projectName: '食谱 App',
  mode: 'pack',
  fastRole: null,
  packName: null,
  railOpen: localStorage.getItem('hexagon.rail') !== '0',
  sideTab: 'artifacts',
  usageRows: [],
  avatars: {},
  tabs: [TIMELINE_TAB],
  activeTab: 'timeline',
  splitOpen: false,
  openTab: (t) =>
    set((s) => ({
      tabs: s.tabs.some((x) => x.id === t.id) ? s.tabs : [...s.tabs, t],
      activeTab: t.id,
    })),
  closeTab: (id) =>
    set((s) => {
      if (id === 'timeline') return {}
      const tabs = s.tabs.filter((t) => t.id !== id)
      const activeTab = s.activeTab === id ? (tabs[tabs.length - 1]?.id ?? 'timeline') : s.activeTab
      return { tabs, activeTab }
    }),
  setActiveTab: (id) => set({ activeTab: id }),
  setSplitOpen: (v) => set({ splitOpen: v }),
  streams: {},
  applyDelta: (d) =>
    set((s) => {
      const streams = { ...s.streams }
      if (d.done) {
        delete streams[d.agent_id]
        return { streams }
      }
      const cur = { ...(streams[d.agent_id] ?? {}) }
      // reset=瞬时重试：本次调用的已收文本作废（重试会重吐全文）
      cur[d.call] = d.reset ? '' : (cur[d.call] ?? '') + d.text
      streams[d.agent_id] = cur
      return { streams }
    }),
  setRailOpen: (v) => {
    localStorage.setItem('hexagon.rail', v ? '1' : '0')
    set({ railOpen: v })
  },
  setSideTab: (t) => set({ sideTab: t }),
  setThemePref: (p) => {
    localStorage.setItem('hexagon.theme', p)
    document.documentElement.dataset.theme = resolve(p)
    set({ themePref: p })
  },
  refresh: async () => {
    // 全量重置边界（挂载/项目切换）：时间线游标归零 → 快通道首拉全量。
    // 切项目必须走这里——invalidate 的增量归并会把两个项目的事件缝一起。
    set({ timeline: [] })
    await useUiStore.getState().invalidate()
  },
  refreshSlow: async (tags) => {
    const want = (t: SlowSlice) => !tags || tags.length === 0 || tags.includes(t)
    const [usage, artifacts, team, autonomy, info] = await Promise.all([
      want('usage') ? api.usage() : Promise.resolve(undefined),
      want('artifacts') ? api.artifacts() : Promise.resolve(undefined),
      want('team') ? api.team() : Promise.resolve(undefined),
      want('info') ? api.autonomy() : Promise.resolve(undefined),
      want('info') ? api.projectInfo().catch(() => null) : Promise.resolve(undefined),
    ])
    let avatars: Record<string, string> | undefined
    if (team) {
      avatars = {}
      await Promise.all(
        team.map(async (m) => {
          const u = await api.agentAvatar(m.id).catch(() => null)
          if (u) avatars![m.id] = u
        }),
      )
    }
    set((s) => ({
      ...(usage ? { usageTotal: usage.total, usageRows: usage.rows } : {}),
      ...(artifacts ? { artifacts } : {}),
      ...(team ? { team, avatars: avatars! } : {}),
      ...(autonomy !== undefined ? { autonomy: autonomy || 'L0' } : {}),
      ...(info !== undefined
        ? {
            mode: info?.mode ?? ('pack' as const),
            fastRole: info?.fastpath_role ?? null,
            packName: info?.pack_name ?? null,
            projectName: info?.name ?? s.projectName,
          }
        : {}),
    }))
  },
  invalidate: async (...tags) => {
    const s = useUiStore.getState()
    await Promise.all([s.refreshFast(), s.refreshSlow(tags)])
  },
  refreshFast: async () => {
    // 游标=末条 event.id（events 表 append-only、查询 ASC + after 排他）。
    const after = useUiStore.getState().timeline.at(-1)?.event.id
    const [stages, pending, items] = await Promise.all([
      api.stageStatus(),
      api.pendingQuestions(),
      api.timeline(after),
    ])
    set((s) => ({
      stages,
      pending,
      // after 在服务端即排他；客户端再挡一层防 mock/实现漂移。
      timeline:
        after == null
          ? items
          : [...s.timeline, ...items.filter((i) => i.event.id > after)],
    }))
  },
}))
