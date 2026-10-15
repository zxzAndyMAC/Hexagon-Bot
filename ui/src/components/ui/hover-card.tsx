// 悬停卡 HoverCard（components A16 补件，gap B6 2026-10-15）：外壳同菜单 / Popover
// （圆角 12、--surface、--shadow-overlay、最小宽 220），只是由悬停 / 聚焦触发。
// 键盘：Radix 触发器聚焦即开、失焦即关；卡内不放可交互控件（名片只读）。
import * as React from "react"
import { HoverCard as HoverCardPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"
import { popSurface } from "@/components/ui/dropdown-menu"

/** 开合延时（ms）：进 300 比 Tooltip（150）慢一拍，扫过成员列表不连闪；出 120 让指针能移进卡内。 */
const OPEN_DELAY = 300
const CLOSE_DELAY = 120

function HoverCard({ openDelay = OPEN_DELAY, closeDelay = CLOSE_DELAY, ...props }: React.ComponentProps<typeof HoverCardPrimitive.Root>) {
  return <HoverCardPrimitive.Root openDelay={openDelay} closeDelay={closeDelay} {...props} />
}

const HoverCardTrigger = HoverCardPrimitive.Trigger

function HoverCardContent({ className, align = "start", sideOffset = 8, ...props }: React.ComponentProps<typeof HoverCardPrimitive.Content>) {
  return (
    <HoverCardPrimitive.Portal>
      <HoverCardPrimitive.Content
        data-slot="hover-card-content"
        align={align}
        sideOffset={sideOffset}
        className={cn(popSurface, "outline-none", className)}
        {...props}
      />
    </HoverCardPrimitive.Portal>
  )
}

export { HoverCard, HoverCardContent, HoverCardTrigger }
