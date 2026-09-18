import { create } from 'zustand'
import {
  api,
  type ArtifactRow,
  type PendingQuestion,
  type StageRow,
  type TeamRow,
  type TimelineItem,
} from './api'

export type ThemePref = 'light' | 'dark' | 'system'

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
  usageTotal: { spent_mc: number; limit_cents: number | null } | null
  autonomy: string
  projectName: string
  railOpen: boolean
  avatars: Record<string, string>
  setThemePref: (p: ThemePref) => void
  setRailOpen: (v: boolean) => void
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
  railOpen: localStorage.getItem('hexagon.rail') !== '0',
  avatars: {},
  setRailOpen: (v) => {
    localStorage.setItem('hexagon.rail', v ? '1' : '0')
    set({ railOpen: v })
  },
  setThemePref: (p) => {
    localStorage.setItem('hexagon.theme', p)
    document.documentElement.dataset.theme = resolve(p)
    set({ themePref: p })
  },
  refresh: async () => {
    const [stages, team, artifacts, timeline, pending, usage, autonomy] = await Promise.all([
      api.stageStatus(),
      api.team(),
      api.artifacts(),
      api.timeline(),
      api.pendingQuestions(),
      api.usage(),
      api.autonomy(),
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
      usageTotal: total ? { spent_mc: total.spent_mc, limit_cents: total.limit_cents } : null,
      autonomy: autonomy || 'L0',
      avatars,
    })
  },
}))
