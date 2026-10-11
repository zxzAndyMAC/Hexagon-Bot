// 加载原语（components A14，feedback#loading）：环形 Ring、像素格 PixelGrid、shimmer 文字。
// 减少动效时全局规则停掉 animation：环停转、像素格停在全亮帧（基础 opacity 1）、shimmer 静止。
import * as React from "react"
import { cn } from "@/lib/utils"

/** 14px 环形：`--line-strong` 轨 + `--ink-2` 顶边，.8s 旋转。`tone="solid"` 用于 primary/ink/danger 按钮内（白顶边）。 */
function Spinner({
  className,
  tone = "default",
  size = 14,
  label,
  ...props
}: React.ComponentProps<"span"> & { tone?: "default" | "solid"; size?: number; label?: string }) {
  return (
    <span
      data-slot="spinner"
      role={label ? "status" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      className={cn(
        "inline-block shrink-0 animate-hx-spin rounded-full border-[1.75px] border-line-strong border-t-ink-2",
        tone === "solid" && "border-white/30 border-t-white",
        className
      )}
      style={{ width: size, height: size }}
      {...props}
    />
  )
}

// Drive 波前延迟：第 n 格按反对角线距左上角的步数 ×.1s
const PX_DELAY = [0, 0.1, 0.2, 0.1, 0.2, 0.3, 0.2, 0.3, 0.4]

/** 3×3 像素格（3px 方点、间隙 1.5，`--ink`）。用于等模型、Agent 忙碌、AI 起草。 */
function PixelGrid({ className, label, ...props }: React.ComponentProps<"span"> & { label?: string }) {
  return (
    <span
      data-slot="pixel-grid"
      role={label ? "status" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      className={cn("inline-grid shrink-0 grid-cols-[repeat(3,3px)] gap-[1.5px]", className)}
      {...props}
    >
      {PX_DELAY.map((d, i) => (
        <i key={i} className="size-[3px] rounded-[.5px] bg-ink animate-hx-px" style={{ animationDelay: `${d}s` }} />
      ))}
    </span>
  )
}

/** shimmer 文字：`--ink-3`→`--ink`→`--ink-3` 渐变裁剪，2s 循环。 */
function Shimmer({ className, ...props }: React.ComponentProps<"span">) {
  return (
    <span
      data-slot="shimmer"
      className={cn(
        "animate-hx-sh bg-[linear-gradient(90deg,var(--ink-3)_0%,var(--ink)_45%,var(--ink-3)_90%)] bg-[length:200%_100%] bg-clip-text text-transparent",
        className
      )}
      {...props}
    />
  )
}

export { PixelGrid, Shimmer, Spinner }
