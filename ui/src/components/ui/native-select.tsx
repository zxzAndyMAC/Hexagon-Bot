// 原生下拉 NativeSelect（components A9「Select 触发」）：外壳与 Input 同一套 fieldShell（34 高、--field 底、
// 内环、聚焦环、禁用），右侧 chevron-down --ink-3。用原生 <select>：选项少、无搜索的表单字段
// （向导角色详情的上级 / 模型槽）不需要 Radix Select 的弹层；键盘与读屏走浏览器原生行为。
import * as React from "react"
import { ChevronDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { fieldShell } from "@/components/ui/input"

function NativeSelect({ className, children, ...props }: React.ComponentProps<"select">) {
  return (
    <div data-slot="native-select" className={cn(fieldShell, "relative h-[34px]", className)}>
      <select
        className="h-full min-w-0 flex-1 cursor-pointer appearance-none bg-transparent pr-7 pl-[11px] text-ink outline-none disabled:cursor-default"
        {...props}
      >
        {children}
      </select>
      <ChevronDown aria-hidden strokeWidth={1.75} className="pointer-events-none absolute right-[11px] size-3.5 text-ink-3" />
    </div>
  )
}

export { NativeSelect }
