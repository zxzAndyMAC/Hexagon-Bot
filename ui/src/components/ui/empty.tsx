// 空态 Empty（components A19，feedback#empty）：说明为什么空 + 给一个动作。
// 结构：居中 padding 28 20；44 圆角 12 图标块（--canvas 底 + --line 内环，--ink-2 图标）；
// 14/600 标题；13 --ink-3 说明（最宽 280）；一个动作按钮（sm）。
// sm（补设计，票 13）：侧栏/选择器等窄容器内的小号——padding 16 12、32 图标块圆角 9、13/600 标题、12.5 说明。
// 加载用 A20 骨架，失败用 C5——不伪装为空；需先打开项目时动作改为「打开项目」，不禁用。
import type { ReactNode } from "react"
import { cn } from "@/lib/utils"
import { Icon, type IconName } from "@/components/Icon"

function Empty({ icon = "inbox", title, desc, action, size = "md", className, ...rest }: {
  icon?: IconName
  title: string
  desc?: ReactNode
  action?: ReactNode
  size?: "md" | "sm"
  className?: string
} & Record<`data-${string}`, string | boolean | undefined>) {
  const sm = size === "sm"
  return (
    <div data-empty data-slot="empty" className={cn("flex flex-col items-center text-center", sm ? "px-3 py-4" : "px-5 py-7", className)} {...rest}>
      <span className={cn("grid place-items-center bg-canvas text-ink-2 shadow-[inset_0_0_0_1px_var(--line)]", sm ? "mb-2 size-8 rounded-[9px]" : "mb-3 size-11 rounded-[12px]")}>
        <Icon name={icon} size={sm ? 16 : 20} />
      </span>
      <div className={cn("font-semibold text-ink", sm ? "text-[13px]" : "text-[14px]")}>{title}</div>
      {desc && <div className={cn("mt-1 max-w-[280px] text-ink-3", sm ? "text-[12.5px]" : "text-[13px]")}>{desc}</div>}
      {action && <div className={cn("flex justify-center gap-1.5", sm ? "mt-2.5" : "mt-3.5")}>{action}</div>}
    </div>
  )
}

export { Empty }
