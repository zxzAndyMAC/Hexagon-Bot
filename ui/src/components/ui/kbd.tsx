// 键位 Kbd（components A3）：18 高、圆角 5、10.5/500 等宽、--ink-3 字、--canvas 底、--line 内环。
// 按钮内（primary/ink/danger）与 tooltip 内两种变体靠所在容器自动切换，调用处不传参。
import * as React from "react"
import { cn } from "@/lib/utils"
import { formatBinding } from "@/keymap"

function Kbd({ className, ...props }: React.ComponentProps<"kbd">) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        "pointer-events-none inline-flex h-[18px] w-fit shrink-0 select-none items-center gap-px rounded-[5px] bg-canvas px-[5px] font-mono text-[10.5px] font-medium text-ink-3 shadow-[inset_0_0_0_1px_var(--line)]",
        "on-solid:bg-[oklch(100%_0_0/.18)] on-solid:text-inherit on-solid:opacity-90 on-solid:shadow-none",
        "in-tip:bg-[oklch(100%_0_0/.12)] in-tip:text-inherit in-tip:shadow-none",
        className
      )}
      {...props}
    />
  )
}

function KbdGroup({ className, ...props }: React.ComponentProps<"kbd">) {
  return <kbd data-slot="kbd-group" className={cn("inline-flex items-center gap-0.5", className)} {...props} />
}

/** 规范形绑定（`mod+B`）→ 按平台渲染 ⌘B / Ctrl+B（ADR 0051）。 */
function ShortcutKbd({ binding, className }: { binding: string; className?: string }) {
  return <Kbd className={className}>{formatBinding(binding)}</Kbd>
}

export { Kbd, KbdGroup, ShortcutKbd }
