// 用量详情的小件（票 12）：图表卡 E13 外壳、统计卡 E12、榜单行 hbar、加载骨架 A20、空态 A19、行内错误 C5（块形）。
// A19 / A20 / C5 已改用票 13 的共享件（ui/empty、ui/skeleton、ui/inline-error）。
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Empty } from '@/components/ui/empty'
import { InlineError } from '@/components/ui/inline-error'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

/** E13 chart-card：头 chart-h（标题 14/600 + 说明 12.5 ink-3 + 右侧控件）/ 体 / 尾 chart-f。 */
export function ChartCard({ title, desc, right, footer, children, className, ...rest }: {
  title: string
  desc?: ReactNode
  right?: ReactNode
  footer?: ReactNode
  children: ReactNode
  className?: string
} & Record<`data-${string}`, string | undefined>) {
  return (
    <Card className={cn('overflow-hidden rounded-[12px]', className)} {...rest}>
      <div className="flex items-center gap-3 border-b border-line-soft px-[18px] py-4">
        <div className="min-w-0 flex-1">
          <h4 className="text-[14px] font-semibold text-ink">{title}</h4>
          {desc && <p className="text-[12.5px] text-ink-3">{desc}</p>}
        </div>
        {right}
      </div>
      {children}
      {footer && <div className="border-t border-line-soft px-[18px] pt-3 pb-4 text-[12.5px]">{footer}</div>}
    </Card>
  )
}

/** E12 统计卡：键 12 ink-3 / 值 22/600 tabular / 附 12 ink-3。 */
export function StatCard({ k, v, sub, action, children, ...rest }: {
  k: string
  v: ReactNode
  sub?: ReactNode
  action?: ReactNode
  children?: ReactNode
} & Record<`data-${string}`, string | undefined>) {
  return (
    <Card className="px-4 py-3.5" {...rest}>
      <div className="flex min-h-6 items-center gap-2">
        <span className="flex-1 text-[12px] text-ink-3">{k}</span>
        {action}
      </div>
      <div className="mt-0.5 text-[22px] font-semibold tracking-[-0.02em] text-ink tabular-nums">{v}</div>
      {sub && <div className="text-[12px] text-ink-3">{sub}</div>}
      {children}
    </Card>
  )
}

/** 「↑ 18%」：上升 green、下降 ink-2，箭头 + 文字，不只靠颜色（E12）。 */
export function Delta({ pct }: { pct: number }) {
  const up = pct >= 0
  return (
    <span className={cn('font-medium', up ? 'text-green' : 'text-ink-2')}>
      {up ? '↑' : '↓'} {Math.abs(pct).toFixed(Math.abs(pct) < 10 ? 1 : 0)}%
    </span>
  )
}

/** E12 榜单行 hbar：grid 104 / 1fr / 64，30 高；8 高条；数值等宽右对齐；悬停 tooltip 给精确值（title）。 */
export function HBar({ label, value, max, color, exact, muted = false }: {
  label: ReactNode
  value: string
  max: number
  color: string
  exact: string
  muted?: boolean
}) {
  return (
    <div data-hbar title={exact} className="grid h-[30px] grid-cols-[104px_1fr_64px] items-center gap-2.5 text-[13px]">
      <span className={cn('flex min-w-0 items-center gap-1.5 truncate', muted ? 'text-ink-3' : 'text-ink')}>{label}</span>
      <span className="h-2 overflow-hidden rounded-full bg-canvas">
        <i className="block h-full rounded-[inherit]" style={{ width: `${Math.max(2, max)}%`, background: color }} />
      </span>
      <span className="text-right font-mono text-[12px] text-ink tabular-nums">{value}</span>
    </div>
  )
}

/** A20 骨架（共享件 ui/skeleton，票 13）：三条自底向上；容器 aria-busy。 */
export function ChartSkeleton({ height }: { height: number }) {
  const { t } = useTranslation()
  return (
    <div role="status" aria-busy="true" aria-label={t('usageDetail.loading')} data-usage-loading className="flex flex-col justify-end gap-2" style={{ height }}>
      {[0.55, 0.8, 1].map((w) => <Skeleton key={w} className="h-3" style={{ width: `${w * 100}%` }} />)}
    </div>
  )
}

/** A19 空态（共享件 ui/empty，票 13）。 */
export function UsageEmpty({ onAction, actionLabel }: { onAction?: () => void; actionLabel?: string }) {
  const { t } = useTranslation()
  return (
    <Empty data-usage-empty icon="chart-column" title={t('usageDetail.empty')} desc={t('usageDetail.emptyHint')}
      action={onAction && actionLabel && <Button size="sm" onClick={onAction}>{actionLabel}</Button>} />
  )
}

/** C5 块形（共享件 ui/inline-error，票 13）：临时性错误给「重试」。 */
export function UsageLoadError({ onRetry, retrying }: { onRetry: () => void; retrying?: boolean }) {
  const { t } = useTranslation()
  return <InlineError data-usage-error title={t('usageDetail.loadFailed')} onRetry={onRetry} retryLabel={t('usageDetail.retry')} retrying={retrying} />
}
