// 图表 Chart（components E13，usage#detail / usage#agents / usage#quick）：shadcn Chart（Recharts）按 Hexagon token 改写。
// 只画横向网格 --grid；X 刻度 11 ink-3；悬停竖向虚线参考线 --line-strong 3 3；
// 提示 ctip：最小 150、圆角 9、overlay 面、日期 500 + 每行色点 + 名 + 等宽值；图例下方居中 12 ink-2（8 方块圆角 2）。
// 票 12：ECharts 迁到 Recharts（spec「图表从 ECharts 迁到 shadcn Chart」）。颜色直接写 chart-1..3 / agent-c1..8 的 CSS 变量，
// 深浅两主题由 CSS 变量本身切换，不走 shadcn 的 THEMES 选择器（应用用 html[data-theme]，不是 .dark）。
import * as React from "react"
import * as RechartsPrimitive from "recharts"
import { cn } from "@/lib/utils"

export type ChartConfig = Record<string, { label: React.ReactNode; color: string }>

type ChartContextProps = { config: ChartConfig }

const ChartContext = React.createContext<ChartContextProps | null>(null)

function useChart() {
  const context = React.useContext(ChartContext)
  if (!context) throw new Error("useChart must be used within a <ChartContainer />")
  return context
}

const INITIAL_DIMENSION = { width: 320, height: 200 } as const

function ChartContainer({
  className,
  children,
  config,
  initialDimension = INITIAL_DIMENSION,
  ...props
}: React.ComponentProps<"div"> & {
  config: ChartConfig
  children: React.ComponentProps<typeof RechartsPrimitive.ResponsiveContainer>["children"]
  initialDimension?: { width: number; height: number }
}) {
  return (
    <ChartContext.Provider value={{ config }}>
      <div
        data-slot="chart"
        className={cn(
          "flex w-full justify-center text-[11px]",
          "[&_.recharts-cartesian-axis-tick_text]:fill-ink-3 [&_.recharts-cartesian-grid_line]:stroke-[var(--grid)]",
          "[&_.recharts-curve.recharts-tooltip-cursor]:stroke-line-strong [&_.recharts-dot[stroke='#fff']]:stroke-transparent",
          "[&_.recharts-layer]:outline-hidden [&_.recharts-sector]:outline-hidden [&_.recharts-surface]:outline-hidden",
          // E13 聚焦：图表容器（Recharts 的 svg，accessibilityLayer）可聚焦，焦点环画在外框上
          "rounded-[var(--r-sm)] has-[.recharts-surface:focus-visible]:shadow-[0_0_0_2px_var(--surface),0_0_0_4px_var(--accent)]",
          className
        )}
        {...props}
      >
        <RechartsPrimitive.ResponsiveContainer initialDimension={initialDimension}>
          {children}
        </RechartsPrimitive.ResponsiveContainer>
      </div>
    </ChartContext.Provider>
  )
}

const ChartTooltip = RechartsPrimitive.Tooltip

/** 提示与图例都按 config 声明序倒排（声明自下而上 → 读自上而下，堆叠顶层在前）；Recharts 的 payload 顺序不可靠。 */
function byConfigTopDown<T extends { dataKey?: unknown; value?: unknown; type?: string }>(payload: ReadonlyArray<T>, config: ChartConfig): T[] {
  const order = Object.keys(config)
  const rank = (p: T) => order.indexOf(String(p.dataKey ?? p.value ?? ""))
  return payload.filter((p) => p.type !== "none").sort((a, b) => rank(b) - rank(a))
}

type TooltipItem = { dataKey?: unknown; name?: unknown; value?: unknown; color?: string; type?: string; payload?: { fill?: string } }

/** ctip：日期 500 + 每行色点 + 名 + 等宽值（E13）。值格式由调用方给（token 走 fmtTok）。 */
function ChartTooltipContent({
  active,
  payload,
  label,
  labelFormatter,
  valueFormatter = (v) => v.toLocaleString(),
  hideLabel = false,
  className,
}: {
  active?: boolean
  payload?: ReadonlyArray<TooltipItem>
  label?: React.ReactNode
  labelFormatter?: (label: React.ReactNode) => React.ReactNode
  valueFormatter?: (v: number) => string
  hideLabel?: boolean
  className?: string
}) {
  const { config } = useChart()
  if (!active || !payload?.length) return null
  // 堆叠图里 Recharts 的 payload 自下而上；提示从上往下读，与视觉层次一致
  const rows = byConfigTopDown(payload, config)
  return (
    <div
      data-slot="chart-tooltip"
      className={cn("grid min-w-[150px] gap-1 rounded-[9px] bg-surface px-2.5 py-2 text-[12px] text-ink shadow-[var(--shadow-overlay)]", className)}
    >
      {!hideLabel && label != null && <div className="font-medium">{labelFormatter ? labelFormatter(label) : label}</div>}
      {rows.map((item) => {
        const key = String(item.dataKey ?? item.name ?? "value")
        const conf = config[key]
        return (
          <div key={key} className="flex items-center gap-2 text-ink-2">
            <i aria-hidden className="size-2 shrink-0 rounded-full" style={{ background: item.color ?? item.payload?.fill ?? conf?.color }} />
            <span className="min-w-0 truncate">{conf?.label ?? String(item.name ?? key)}</span>
            <b className="ml-auto pl-3 font-mono text-[11.5px] font-medium text-ink tabular-nums">
              {typeof item.value === "number" ? valueFormatter(item.value) : String(item.value ?? "")}
            </b>
          </div>
        )
      })}
    </div>
  )
}

const ChartLegend = RechartsPrimitive.Legend

/** 图例：下方居中 12 ink-2，8 方块圆角 2（E13）。 */
function ChartLegendContent({ payload, className }: { payload?: ReadonlyArray<{ dataKey?: unknown; value?: unknown; color?: string; type?: string }>; className?: string }) {
  const { config } = useChart()
  if (!payload?.length) return null
  // 与提示同序：自上而下（堆叠顶层在前）
  const items = byConfigTopDown(payload, config)
  return (
    <div data-slot="chart-legend" className={cn("flex flex-wrap items-center justify-center gap-4 pt-1.5 pb-3.5 text-[12px] text-ink-2", className)}>
      {items.map((item) => {
        const key = String(item.dataKey ?? item.value ?? "")
        return (
          <span key={key} className="inline-flex items-center gap-1.5">
            <i aria-hidden className="size-2 shrink-0 rounded-[2px]" style={{ background: item.color ?? config[key]?.color }} />
            {config[key]?.label ?? String(item.value ?? key)}
          </span>
        )
      })}
    </div>
  )
}

export { ChartContainer, ChartTooltip, ChartTooltipContent, ChartLegend, ChartLegendContent, useChart }
