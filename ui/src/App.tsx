import { useEffect, useState } from 'react'
import './i18n'
import { TopBar } from './components/TopBar'
import { StageBar } from './components/StageBar'
import { Timeline } from './components/Timeline'
import { TabBar } from './components/TabBar'
import { ArtifactTab } from './components/ArtifactTab'
import { AgentTab } from './components/AgentTab'
import { UsageDetailTab } from './components/UsageDetailTab'
import { DiffView } from './components/DiffView'
import { parseUnifiedDiff } from './diff'
import { SidePanel } from './components/SidePanel'
import { Composer } from './components/Composer'
import { SettingsDrawer } from './components/SettingsDrawer'
import { useUiStore } from './store'
import { PendingCards } from './components/PendingCards'
import { usePendingKeys } from './decisions'
import { bindingFor, matches } from './keymap'

export default function App() {
  const refresh = useUiStore((s) => s.refresh)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const { onKey } = usePendingKeys()

  useEffect(() => {
    refresh()
    const iv = setInterval(refresh, 2000) // 事件推送落地前的轮询占位
    return () => clearInterval(iv)
  }, [refresh])

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return
      if (matches(e, bindingFor('toggleRail'))) {
        e.preventDefault()
        useUiStore.getState().setRailOpen(!useUiStore.getState().railOpen)
        return
      }
      if (matches(e, bindingFor('splitEditor'))) {
        e.preventDefault()
        useUiStore.getState().setSplitOpen(!useUiStore.getState().splitOpen)
        return
      }
      if (matches(e, bindingFor('closeTab'))) {
        e.preventDefault()
        const s = useUiStore.getState()
        s.closeTab(s.activeTab) // timeline tab 在 closeTab 内被拦
        return
      }
      void onKey(e)
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [onKey])

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <TopBar onSettings={() => setSettingsOpen(true)} />
      <StageBar />
      <div style={{ flex: 1, minHeight: 0, display: 'flex', gap: 10, padding: '10px 14px' }}>
        <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
          <div id="pending-zone" style={{ maxHeight: 220, overflowY: 'auto', flexShrink: 0 }}>
            <PendingCards />
          </div>
          <TabBar />
          <CenterPanes />
          <Composer />
        </div>
        <SidePanel />
      </div>
      <SettingsDrawer open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  )
}

function TabContent({ tab }: { tab: ReturnType<typeof useUiStore.getState>['tabs'][number] }) {
  switch (tab.kind) {
    case 'artifact':
      return <ArtifactTab path={tab.path!} />
    case 'agent':
      return <AgentTab agentId={tab.agentId!} />
    case 'usage':
      return <UsageDetailTab />
    case 'diff':
      return (
        <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
          <div className="row-line mono" style={{ padding: '8px 14px', fontSize: 12, fontWeight: 560 }}>
            ± {tab.title}
          </div>
          <DiffView ops={parseUnifiedDiff(tab.patchText ?? '')} />
        </div>
      )
    default:
      return <Timeline />
  }
}

function CenterPanes() {
  const { tabs, activeTab, splitOpen } = useUiStore()
  const active = tabs.find((t) => t.id === activeTab) ?? tabs[0]
  // 分屏右栏默认给「另一个最近 tab」；本地状态记右栏选中
  const [splitId, setSplitId] = useState<string | null>(null)
  const others = tabs.filter((t) => t.id !== active.id)
  const splitTab = others.find((t) => t.id === splitId) ?? others[others.length - 1] ?? tabs[0]
  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex' }}>
      <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        {tabs.map((t) => (
          <div key={t.id} style={{ display: t.id === active.id ? 'flex' : 'none', flex: 1, minHeight: 0, flexDirection: 'column' }}>
            <TabContent tab={t} />
          </div>
        ))}
      </div>
      {splitOpen && (
        <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', borderLeft: '1px solid var(--border-strong)' }}>
          <div className="row-line" style={{ display: 'flex', gap: 4, padding: '4px 8px', alignItems: 'center' }}>
            {tabs.map((t) => (
              <button
                key={t.id}
                className={`chip ${t.id === splitTab.id ? 'amber' : ''}`}
                style={{ cursor: 'pointer', fontSize: 10 }}
                onClick={() => setSplitId(t.id)}
              >
                {t.kind === 'agent' ? (t.role ?? t.title) : t.kind === 'timeline' ? '≣' : t.title}
              </button>
            ))}
          </div>
          <TabContent tab={splitTab} />
        </div>
      )}
    </div>
  )
}
