// 面积趋势图（components E13 体区 chart-b；usage#detail 堆叠 · usage#agents 不堆叠 · usage#quick 迷你）。
// 曲线 natural；只画横向网格；无轴线、无 Y 轴；X 刻度 minTickGap 32。
// 堆叠：渐变 5% .8 → 95% .1、描边 1.25；不堆叠：.22 → 0、描边 1.75。悬停参考线 + 每层 3.5 圆点（2px --surface 描边）。
// 聚焦：Recharts accessibilityLayer 让 svg 可 Tab 到，←→ 移动提示点；role="img" + aria-label 概述（E13 聚焦行）。
import { useId } from 'react'
import { Area, AreaChart, CartesianGrid, XAxis } from 'recharts'
import { ChartContainer, ChartLegend, ChartLegendContent, ChartTooltip, ChartTooltipContent, type ChartConfig } from '@/components/ui/chart'
import { fmtTok } from '../../usage'

export type TrendSeries = { key: string; label: string; color: string }

export function AreaTrend({
  data, series, stacked, height, ariaLabel, tick, tooltipLabel, mini = false,
}: {
  data: ({ bucket: string } & Record<string, number | string>)[]
  /** 自下而上（堆叠时第一项在最底层）。 */
  series: TrendSeries[]
  stacked: boolean
  height: number
  ariaLabel: string
  tick?: (bucket: string) => string
  tooltipLabel?: (bucket: string) => string
  /** 迷你版（用量卡）：无刻度、无图例、无提示。 */
  mini?: boolean
}) {
  const gid = useId().replace(/:/g, '')
  const config: ChartConfig = Object.fromEntries(series.map((s) => [s.key, { label: s.label, color: s.color }]))
  return (
    <ChartContainer config={config} style={{ height }} data-area-trend={stacked ? 'stacked' : 'lines'}>
      <AreaChart data={data} accessibilityLayer={!mini} role="img" aria-label={ariaLabel} margin={{ top: 4, right: mini ? 0 : 8, bottom: 0, left: mini ? 0 : 8 }}>
        <defs>
          {series.map((s) => (
            <linearGradient key={s.key} id={`${gid}-${s.key}`} x1="0" y1="0" x2="0" y2="1">
              <stop offset="5%" stopColor={s.color} stopOpacity={stacked ? 0.8 : 0.22} />
              <stop offset="95%" stopColor={s.color} stopOpacity={stacked ? 0.1 : 0} />
            </linearGradient>
          ))}
        </defs>
        {!mini && <CartesianGrid vertical={false} />}
        {!mini && (
          <XAxis dataKey="bucket" tickLine={false} axisLine={false} tickMargin={8} minTickGap={32}
            tickFormatter={(v: string) => (tick ? tick(v) : v)} />
        )}
        {!mini && (
          <ChartTooltip
            cursor={{ stroke: 'var(--line-strong)', strokeDasharray: '3 3' }}
            content={<ChartTooltipContent valueFormatter={fmtTok} labelFormatter={(l) => (tooltipLabel ? tooltipLabel(String(l)) : l)} />}
          />
        )}
        {series.map((s) => (
          <Area
            key={s.key}
            dataKey={s.key}
            name={s.label}
            type="natural"
            stackId={stacked ? 'a' : undefined}
            stroke={s.color}
            strokeWidth={stacked ? 1.25 : 1.75}
            fill={`url(#${gid}-${s.key})`}
            isAnimationActive={false}
            activeDot={mini ? false : { r: 3.5, stroke: 'var(--surface)', strokeWidth: 2, fill: s.color }}
          />
        ))}
        {!mini && <ChartLegend verticalAlign="bottom" content={<ChartLegendContent />} />}
      </AreaChart>
    </ChartContainer>
  )
}
