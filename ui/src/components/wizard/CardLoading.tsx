// 卡位加载（components A14 / E10「出题 / 起草：卡位像素格」）：像素格 + shimmer 文案 + 等宽计时，role=status。
// 等模型用像素格（interaction §13）；向导里答问卡、草稿框、职责框在途时原地换成它。
import { useEffect, useState } from 'react'
import { cn } from '@/lib/utils'
import { PixelGrid, Shimmer } from '@/components/ui/spinner'

export function CardLoading({ label, className }: { label: string; className?: string }) {
  const [start] = useState(() => Date.now())
  const [now, setNow] = useState(start)
  useEffect(() => {
    const iv = setInterval(() => setNow(Date.now()), 200)
    return () => clearInterval(iv)
  }, [])
  return (
    <div
      role="status"
      className={cn('flex min-h-[120px] items-center justify-center gap-2.5 rounded-[12px] bg-inset text-[13px] shadow-[inset_0_0_0_1px_var(--line-soft)]', className)}
    >
      <PixelGrid />
      <Shimmer>{label}</Shimmer>
      <span className="font-mono text-[12px] text-ink-3">{((now - start) / 1000).toFixed(1)}s</span>
    </div>
  )
}
