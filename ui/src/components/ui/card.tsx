// 表面 Card / Raised / Overlay / Inset（components A8，index#type「层级」）：分层靠环线阴影，不用 border。
// card = --app-card + --shadow-card + 10；raised = --surface + --shadow-raised + 10；
// overlay = --surface + --shadow-overlay + 12；inset = --inset + --line-soft 内环 + 10。
// 可点击卡用 CardButton（悬停 --hover 底、按下、焦点环）；纯展示卡 Card 无交互态。
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

const surfaceVariants = cva("flex flex-col text-ink", {
  variants: {
    surface: {
      card: "rounded-[var(--r-card)] bg-[var(--app-card)] [--hx-shadow:var(--shadow-card)]",
      raised: "rounded-[var(--r-card)] bg-surface [--hx-shadow:var(--shadow-raised)]",
      overlay: "rounded-[12px] bg-surface [--hx-shadow:var(--shadow-overlay)]",
      inset: "rounded-[var(--r-card)] bg-inset [--hx-shadow:inset_0_0_0_1px_var(--line-soft)]",
    },
    interactive: {
      true: "hx-press hx-focus cursor-pointer text-left live:hover:bg-hover",
      false: "shadow-[var(--hx-shadow)]",
    },
  },
  defaultVariants: { surface: "card", interactive: false },
})

type CardProps = React.ComponentProps<"div"> & Pick<VariantProps<typeof surfaceVariants>, "surface">

function Card({ className, surface, ...props }: CardProps) {
  return <div data-slot="card" data-surface={surface ?? "card"} className={cn(surfaceVariants({ surface }), className)} {...props} />
}

/** 可点击卡（交付卡、最近项目行）：悬停 --hover 底、按下、焦点环。 */
function CardButton({
  className,
  surface,
  type = "button",
  ...props
}: React.ComponentProps<"button"> & Pick<VariantProps<typeof surfaceVariants>, "surface">) {
  return (
    <button
      type={type}
      data-slot="card"
      data-surface={surface ?? "card"}
      className={cn(surfaceVariants({ surface, interactive: true }), className)}
      {...props}
    />
  )
}

function CardHeader({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="card-header" className={cn("flex items-center gap-2.5 px-4 pt-3.5", className)} {...props} />
}

/** 卡片标题：14/600（foundations §2「卡片标题」）。 */
function CardTitle({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="card-title" className={cn("text-[14px] font-semibold", className)} {...props} />
}

function CardDescription({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="card-description" className={cn("text-[12.5px] text-ink-2", className)} {...props} />
}

function CardContent({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="card-content" className={cn("px-4 py-3 text-[13px]", className)} {...props} />
}

function CardFooter({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="card-footer" className={cn("flex items-center justify-end gap-2 px-4 pb-3.5", className)} {...props} />
}

export { Card, CardButton, CardContent, CardDescription, CardFooter, CardHeader, CardTitle, surfaceVariants }
