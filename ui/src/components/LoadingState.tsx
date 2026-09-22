// LoadingState 移植（beautiful-ui 票 04，借形 MIT slev12397/beautiful-ui）：
// Drive 变体——3×3 方格 chevron 波前 + shimmer 标签 + mono 计时。
// 只搬 Drive；Dots/Orbit 只是 delay 表不同，要换再扩 PATTERNS。
// reduced-motion 由 index.css 全局熔断（格子冻结 dim），计时器照走不丢信息。
import { useEffect, useState } from 'react'

// chevron 波前：每格点亮延时 = (列 + |行-1|) × 90ms。
// 650ms 周期 < 720ms 全程扫宽——两个波前始终在飞（原作设计意图）。
const CHEVRON = Array.from({ length: 9 }, (_, i) => {
  const r = Math.floor(i / 3)
  const c = i % 3
  return (c + Math.abs(r - 1)) * 90
})

function fmtElapsed(ms: number): string {
  const s = Math.max(0, ms) / 1000
  if (s < 60) return `${s.toFixed(1)}s`
  const m = Math.floor(s / 60)
  return `${m}m ${(s % 60).toFixed(1)}s`
}

export function LoadingState({ label, startedAt }: { label: string; startedAt?: number }) {
  // startedAt 缺省=挂载时刻（等待起点无从追溯时的诚实兜底）
  const [start] = useState(() => startedAt ?? Date.now())
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const iv = setInterval(() => setNow(Date.now()), 200)
    return () => clearInterval(iv)
  }, [])
  return (
    <span role="status" className="loading-state">
      <span aria-hidden className="loading-grid">
        {CHEVRON.map((d, i) => (
          <span
            key={i}
            className="loading-cell"
            style={{ animationDelay: `${d}ms` }}
          />
        ))}
      </span>
      <span className="shimmer-text">{label}</span>
      <span className="loading-elapsed mono dim3">{fmtElapsed(now - start)}</span>
    </span>
  )
}
