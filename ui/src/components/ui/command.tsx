// 命令面板 Command（components D18，shadcn Command = cmdk）：放在共享 Dialog 里用。
// 2026-10-14 负责人裁决引入 cmdk（票 07 原自绘 listbox 被否）：键盘导航、选中态、option 语义交给 cmdk；
// 外观仍按 D18 / A16：搜索行 48 高（search 16 + 输入 15px + 右侧插槽放 Esc kbd）+ 底分隔线；
// 组标题 11.5 ink-3；行 32 高、圆角 7、13/500，键盘高亮与悬停同为 --hover 底（cmdk data-selected）。
// shadcn 生成版的 InputGroup 输入框、行尾 ✓ 不用（D18 行尾是灰描述 + kbd）。
import * as React from "react"
import { Command as CommandPrimitive } from "cmdk"
import { Search } from "lucide-react"
import { cn } from "@/lib/utils"

function Command({ className, ...props }: React.ComponentProps<typeof CommandPrimitive>) {
  return (
    <CommandPrimitive data-slot="command" className={cn("flex size-full flex-col overflow-hidden text-ink", className)} {...props} />
  )
}

function CommandInput({ className, trailing, ...props }: React.ComponentProps<typeof CommandPrimitive.Input> & { trailing?: React.ReactNode }) {
  return (
    <div data-slot="command-input-wrapper" className="flex h-12 items-center gap-2.5 border-b border-line px-3.5">
      <Search size={16} strokeWidth={1.75} className="shrink-0 text-ink-3" aria-hidden />
      <CommandPrimitive.Input
        data-slot="command-input"
        className={cn("min-w-0 flex-1 border-0 bg-transparent text-[15px] text-ink outline-none placeholder:text-ink-3", className)}
        {...props}
      />
      {trailing}
    </div>
  )
}

function CommandList({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.List>) {
  return (
    <CommandPrimitive.List data-slot="command-list"
      className={cn("max-h-[min(420px,56vh)] scroll-py-1.5 overflow-y-auto p-1.5 outline-none", className)} {...props} />
  )
}

function CommandEmpty({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.Empty>) {
  return <CommandPrimitive.Empty data-slot="command-empty" className={cn("px-4 py-6 text-center text-[13px] text-ink-3", className)} {...props} />
}

function CommandGroup({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.Group>) {
  return (
    <CommandPrimitive.Group data-slot="command-group"
      className={cn("**:[[cmdk-group-heading]]:px-[9px] **:[[cmdk-group-heading]]:pt-2 **:[[cmdk-group-heading]]:pb-1 **:[[cmdk-group-heading]]:text-[11.5px] **:[[cmdk-group-heading]]:text-ink-3", className)}
      {...props} />
  )
}

function CommandItem({ className, ...props }: React.ComponentProps<typeof CommandPrimitive.Item>) {
  return (
    <CommandPrimitive.Item data-slot="command-item"
      className={cn("flex h-8 cursor-pointer items-center gap-[9px] rounded-[7px] px-[9px] text-[13px] font-medium text-ink outline-none select-none data-[disabled=true]:pointer-events-none data-[disabled=true]:opacity-50 data-[selected=true]:bg-hover", className)}
      {...props} />
  )
}

export { Command, CommandInput, CommandList, CommandEmpty, CommandGroup, CommandItem }
