// 胶囊 Chip / Badge（components A4）：22 高、padding 0 8、pill（sq 变体 6）、12px/500，可带 12px 图标或 7px 点。
// 只表状态，不可点。浅色语义胶囊带同色细描边（foundations §1.3 的 N%），深色 --tint-ring=0 自动去掉。
// 描边类名必须写成字面量：Tailwind 只扫源码文本，拼接出来的类不会生成。
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"
import { Spinner } from "@/components/ui/spinner"

const badgeVariants = cva(
  "inline-flex h-[22px] w-fit shrink-0 items-center gap-[5px] whitespace-nowrap px-2 text-[12px] font-medium shadow-[var(--hx-shadow)] [&>svg]:pointer-events-none [&>svg:not([class*='size-'])]:size-3",
  {
    variants: {
      variant: {
        neutral: "bg-canvas text-ink-2",
        ok: cn("bg-green-tint text-green-ink", "[--hx-shadow:inset_0_0_0_1px_color-mix(in_oklch,var(--green)_calc(28%*var(--tint-ring)),transparent)]"),
        warn: cn("bg-orange-tint text-orange-ink", "[--hx-shadow:inset_0_0_0_1px_color-mix(in_oklch,var(--orange)_calc(30%*var(--tint-ring)),transparent)]"),
        err: cn("bg-red-tint text-red-ink", "[--hx-shadow:inset_0_0_0_1px_color-mix(in_oklch,var(--red)_calc(26%*var(--tint-ring)),transparent)]"),
        info: cn("bg-accent-tint text-accent-ink", "[--hx-shadow:inset_0_0_0_1px_color-mix(in_oklch,var(--accent)_calc(24%*var(--tint-ring)),transparent)]"),
        flag: cn("bg-violet-tint text-violet-ink", "[--hx-shadow:inset_0_0_0_1px_color-mix(in_oklch,var(--violet)_calc(24%*var(--tint-ring)),transparent)]"),
        line: "bg-transparent text-ink-2 [--hx-shadow:inset_0_0_0_1px_var(--line-strong)]",
      },
      shape: { pill: "rounded-full", sq: "rounded-[6px]" },
    },
    defaultVariants: { variant: "neutral", shape: "pill" },
  }
)

function Badge({
  className,
  variant = "neutral",
  shape = "pill",
  loading = false,
  children,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants> & { loading?: boolean }) {
  return (
    <span data-slot="badge" data-variant={variant} className={cn(badgeVariants({ variant, shape }), className)} {...props}>
      {loading && <Spinner size={12} />}
      {children}
    </span>
  )
}

export { Badge, badgeVariants }
