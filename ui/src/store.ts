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
    const [stages, team, artifacts, timeline, pending, usage, autonomy, info] = await Promise.all([
      api.stageStatus(),
      api.team(),
      api.artifacts(),
      api.timeline(),
      api.pendingQuestions(),
      api.usage(),
      api.autonomy(),
      api.projectInfo().catch(() => null),
    ])
    const total = usage.find((r) => r._total)
    const avatars: Record<string, string> = {}
    await Promise.all(
      team.map(async (m) => {
        const u = await api.agentAvatar(m.id).catch(() => null)
        if (u) avatars[m.id] = u
      }),
    )
    set({
      stages,
      team,
      artifacts,
      timeline,
      pending,
      usageTotal: total
        ? { spent_mc: total.spent_mc ?? 0, limit_cents: total.limit_cents ?? null, tokens: total.tokens ?? 0 }
        : null,
      usageRows: usage,
      autonomy: autonomy || 'L0',
      avatars,
      mode: info?.mode ?? 'pack',
      fastRole: info?.fastpath_role ?? null,
      packName: info?.pack_name ?? null,
      projectName: info?.name ?? useUiStore.getState().projectName,
    })
  },
}))
