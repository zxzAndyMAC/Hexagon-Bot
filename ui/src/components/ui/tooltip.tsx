// Tooltip（components A15）：padding 6 9、圆角 8、--tooltip-bg/--tooltip-fg、12px、--shadow-md；
// 悬停或键盘聚焦 150ms 后出现，Esc 关闭；禁用控件仍出现（写原因）。z 序 25（foundations §4）由 Portal 次序保证。
import * as React from "react"
import { Tooltip as TooltipPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"

// disableHoverableContent：A15「指针移入 tooltip 本身不保持（纯提示）」
function TooltipProvider({ delayDuration = 150, disableHoverableContent = true, ...props }: React.ComponentProps<typeof TooltipPrimitive.Provider>) {
  return <TooltipPrimitive.Provider data-slot="tooltip-provider" delayDuration={delayDuration} disableHoverableContent={disableHoverableContent} {...props} />
}

function Tooltip(props: React.ComponentProps<typeof TooltipPrimitive.Root>) {
  return <TooltipPrimitive.Root data-slot="tooltip" {...props} />
}

function TooltipTrigger(props: React.ComponentProps<typeof TooltipPrimitive.Trigger>) {
  return <TooltipPrimitive.Trigger data-slot="tooltip-trigger" {...props} />
}

function TooltipContent({
  className,
  sideOffset = 6,
  children,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Content>) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        data-slot="tooltip-content"
        sideOffset={sideOffset}
        className={cn(
          "z-[25] inline-flex w-fit max-w-xs items-center gap-1.5 rounded-[8px] bg-tooltip px-[9px] py-1.5 text-[12px] text-tooltip-fg shadow-[var(--shadow-md)]",
          "data-[state=delayed-open]:animate-in data-[state=delayed-open]:fade-in-0 data-closed:animate-out data-closed:fade-out-0",
          className
        )}
        {...props}
      >
        {children}
      </TooltipPrimitive.Content>
    </TooltipPrimitive.Portal>
  )
}

export { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger }
