import { useEffect, useState } from 'react'
import './i18n'
import { TopBar } from './components/TopBar'
import { StageBar } from './components/StageBar'
import { Timeline } from './components/Timeline'
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
          <Timeline />
          <Composer />
        </div>
        <SidePanel />
      </div>
      <SettingsDrawer open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  )
}
