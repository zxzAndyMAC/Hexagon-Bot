// 状态点 Dot（components A6）：7×7 圆，默认 --ink-3。on 绿 · warn 橙 · err 红 · info 蓝；pulse 1.6s 呼吸；
// off = 停用空心（inset 1.5px --line-strong，透明底）。非交互；语义由相邻文字承载。
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

const dotVariants = cva("inline-block size-[7px] shrink-0 rounded-full", {
  variants: {
    tone: {
      neutral: "bg-ink-3",
      on: "bg-green",
      warn: "bg-orange",
      err: "bg-red",
      // 票 09（owner-followups-1015）：bg-primary 改映射到更深的 --accent-strong（主按钮白字底），点仍吃 --accent 基色
      info: "bg-(--accent)",
      off: "bg-transparent shadow-[inset_0_0_0_1.5px_var(--line-strong)]",
    },
    pulse: { true: "animate-hx-pulse", false: "" },
  },
  defaultVariants: { tone: "neutral", pulse: false },
})

function Dot({ className, tone, pulse, ...props }: React.ComponentProps<"span"> & VariantProps<typeof dotVariants>) {
  return <span data-slot="dot" data-tone={tone ?? "neutral"} aria-hidden className={cn(dotVariants({ tone, pulse }), className)} {...props} />
}

export { Dot }
