import { useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useUiStore } from './store'

export default function App() {
  const coreStatus = useUiStore((s) => s.coreStatus)
  const setCoreStatus = useUiStore((s) => s.setCoreStatus)

  useEffect(() => {
    invoke<string>('core_ping')
      .then(setCoreStatus)
      .catch((e) => setCoreStatus(`core unreachable: ${e}`))
  }, [setCoreStatus])

  return (
    <main className="flex h-screen flex-col items-center justify-center gap-4">
      <svg width="44" height="48" viewBox="0 0 22 24">
        <polygon
          points="11,1 21,6.5 21,17.5 11,23 1,17.5 1,6.5"
          fill="none"
          stroke="#e8a33d"
          strokeWidth="2"
        />
        <polygon points="11,7 16,9.7 16,15 11,17.7 6,15 6,9.7" fill="#e8a33d" />
      </svg>
      <h1 className="text-lg font-medium tracking-wide">HEXAGON-BOT</h1>
      <p className="font-mono text-xs text-zinc-400">{coreStatus}</p>
    </main>
  )
}
