// 进度条 Progress（components A12）：6 高、pill、--canvas 轨 + --line-soft 内环；
// 填充默认 --ink，变体 accent / warn（--orange）/ cap（触顶 --red，与用量入口 cap 一致）。
// 无上限时调用方不渲染本条；loading 时轨道显示骨架。
import * as React from "react"
import { Progress as ProgressPrimitive } from "radix-ui"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

const fillVariants = cva("block h-full rounded-[inherit] transition-[width] duration-150 ease-hx", {
  variants: { tone: { ink: "bg-ink", accent: "bg-primary", warn: "bg-orange", cap: "bg-red" } },
  defaultVariants: { tone: "ink" },
})

function Progress({
  className,
  value,
  tone,
  loading = false,
  ...props
}: React.ComponentProps<typeof ProgressPrimitive.Root> & VariantProps<typeof fillVariants> & { loading?: boolean }) {
  const pct = Math.max(0, Math.min(100, value ?? 0))
  return (
    <ProgressPrimitive.Root
      data-slot="progress"
      value={loading ? null : pct}
      aria-busy={loading || undefined}
      className={cn(
        "relative h-1.5 w-full overflow-hidden rounded-full bg-canvas shadow-[inset_0_0_0_1px_var(--line-soft)]",
        loading && "animate-hx-skl bg-[linear-gradient(90deg,var(--canvas)_25%,var(--hover-2)_50%,var(--canvas)_75%)] bg-[length:200%_100%]",
        className
      )}
      {...props}
    >
      {!loading && (
        <ProgressPrimitive.Indicator data-slot="progress-indicator" className={fillVariants({ tone })} style={{ width: `${pct}%` }} />
      )}
    </ProgressPrimitive.Root>
  )
}

export { Progress }
