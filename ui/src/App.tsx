import { useEffect, useState } from 'react'
import './i18n'
import { TopBar } from './components/TopBar'
import { StageBar } from './components/StageBar'
import { Timeline } from './components/Timeline'
import { SidePanel } from './components/SidePanel'
import { Composer } from './components/Composer'
import { SettingsDrawer } from './components/SettingsDrawer'
import { useUiStore } from './store'

export default function App() {
  const refresh = useUiStore((s) => s.refresh)
  const [settingsOpen, setSettingsOpen] = useState(false)

  useEffect(() => {
    refresh()
    const iv = setInterval(refresh, 2000) // 事件推送落地前的轮询占位
    return () => clearInterval(iv)
  }, [refresh])

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <TopBar onSettings={() => setSettingsOpen(true)} />
      <StageBar />
      <div style={{ flex: 1, minHeight: 0, display: 'flex', gap: 10, padding: '10px 14px' }}>
        <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
          <Timeline />
          <Composer />
        </div>
        <SidePanel />
      </div>
      <SettingsDrawer open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  )
}
