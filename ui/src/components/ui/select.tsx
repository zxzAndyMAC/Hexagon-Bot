// 弹层下拉 Select（components A9「Select 触发」+ A16 菜单形，补设计 gap B12 2026-10-15）：
// 触发器与 NativeSelect 同一套 fieldShell（34 高、--field 底、内环、聚焦环、禁用），右侧 chevron-down --ink-3；
// 弹层与 DropdownMenu 共用 popSurface（圆角 12、--surface、--shadow-overlay、padding 5、最小宽 220），
// 行 32 高、圆角 7、13px，选中项右侧 ✓；分组标题 11.5 ink-3；分隔 1px --line。
// 什么时候用它而不是 NativeSelect：选项要带头像 / 图标（原生 <option> 只能放纯文本）。
// 键盘与读屏走 Radix Select：触发器 role=combobox，↑↓ 移动、↵/空格选中、Esc 关闭、首字母跳转。
import * as React from "react"
import { Select as SelectPrimitive } from "radix-ui"
import { Check, ChevronDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { fieldShell } from "@/components/ui/input"
import { popSurface } from "@/components/ui/dropdown-menu"
import { useFloatingZ } from "@/components/ui/layer"

const Select = SelectPrimitive.Root
const SelectGroup = SelectPrimitive.Group
const SelectValue = SelectPrimitive.Value

function SelectTrigger({ className, children, ...props }: React.ComponentProps<typeof SelectPrimitive.Trigger>) {
  return (
    <div data-slot="select" className={cn(fieldShell, "relative h-[34px]", className)}>
      <SelectPrimitive.Trigger
        data-slot="select-trigger"
        className="flex h-full min-w-0 flex-1 cursor-pointer items-center gap-2 bg-transparent pr-7 pl-[11px] text-left text-ink outline-none disabled:cursor-default data-placeholder:text-ink-3 [&>span]:flex [&>span]:min-w-0 [&>span]:items-center [&>span]:gap-2 [&>span]:truncate"
        {...props}
      >
        {children}
      </SelectPrimitive.Trigger>
      <ChevronDown aria-hidden strokeWidth={1.75} className="pointer-events-none absolute right-[11px] size-3.5 text-ink-3" />
    </div>
  )
}

function SelectContent({ className, children, sideOffset = 6, container, ...props }: React.ComponentProps<typeof SelectPrimitive.Content> & {
  /** 弹层挂载点（默认 body）。gap C5 后向导是 16 层、低于弹层 20，不再需要挂进向导；保留给标本页等静态比对。 */
  container?: HTMLElement | null
}) {
  const z = useFloatingZ("pop")
  return (
    <SelectPrimitive.Portal container={container}>
      <SelectPrimitive.Content
        data-slot="select-content"
        position="popper"
        sideOffset={sideOffset}
        className={cn(popSurface, z, "max-h-(--radix-select-content-available-height) w-(--radix-select-trigger-width)", className)}
        {...props}
      >
        <SelectPrimitive.Viewport>{children}</SelectPrimitive.Viewport>
      </SelectPrimitive.Content>
    </SelectPrimitive.Portal>
  )
}

function SelectItem({ className, children, icon, ...props }: React.ComponentProps<typeof SelectPrimitive.Item> & {
  /** 行首头像 / 图标（不进触发器显示值时也照样渲染在行里）。 */
  icon?: React.ReactNode
}) {
  return (
    <SelectPrimitive.Item
      data-slot="select-item"
      className={cn(
        "hx-press relative flex h-8 cursor-pointer items-center gap-[9px] rounded-[7px] px-[9px] text-[13px] outline-none select-none data-highlighted:bg-hover data-disabled:cursor-default data-disabled:opacity-45 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-ink-2",
        className,
      )}
      {...props}
    >
      <SelectPrimitive.ItemText>
        <span className="flex min-w-0 items-center gap-2">
          {icon}
          <span data-select-text="" className="truncate">{children}</span>
        </span>
      </SelectPrimitive.ItemText>
      <SelectPrimitive.ItemIndicator className="ml-auto inline-flex">
        <Check className="text-ink!" />
      </SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  )
}

function SelectLabel({ className, ...props }: React.ComponentProps<typeof SelectPrimitive.Label>) {
  return <SelectPrimitive.Label data-slot="select-label" className={cn("px-[9px] pt-2 pb-1 text-[11.5px] text-ink-3", className)} {...props} />
}

function SelectSeparator({ className, ...props }: React.ComponentProps<typeof SelectPrimitive.Separator>) {
  return <SelectPrimitive.Separator data-slot="select-separator" className={cn("mx-0.5 my-1 h-px bg-line", className)} {...props} />
}

export { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue }
