// 分段控件 Segmented（components A7）：shadcn ToggleGroup（type single）。
// 轨道 --canvas 底、--line-soft 内环、padding 2、pill；项 24 高、padding 0 10、12px --ink-2。
// 当前项 --surface + --shadow-btn + --ink + 500。←→ 在项间移动（Radix roving tabindex）。
// 单选必有一项：再点当前项不取消（Radix 会回传空串，这里吞掉）。
import * as React from "react"
import { ToggleGroup as ToggleGroupPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { guardInert } from "@/lib/inert"

function Segmented({
  className,
  value,
  onValueChange,
  children,
  ...props
}: Omit<React.ComponentProps<typeof ToggleGroupPrimitive.Root>, "type" | "value" | "onValueChange" | "defaultValue"> & {
  value: string
  onValueChange: (v: string) => void
}) {
  return (
    <ToggleGroupPrimitive.Root
      data-slot="segmented"
      type="single"
      value={value}
      onValueChange={(v) => { if (v) onValueChange(v) }}
      className={cn("inline-flex w-fit items-center rounded-full bg-canvas p-0.5 shadow-[inset_0_0_0_1px_var(--line-soft)]", className)}
      {...props}
    >
      {children}
    </ToggleGroupPrimitive.Root>
  )
}

function SegmentedItem({
  className,
  disabledReason,
  disabled,
  onClick,
  ...props
}: React.ComponentProps<typeof ToggleGroupPrimitive.Item> & { disabledReason?: string }) {
  const item = (
    <ToggleGroupPrimitive.Item
      data-slot="segmented-item"
      aria-disabled={disabled || undefined}
      onClick={guardInert(disabled, onClick)}
      className={cn(
        "hx-press hx-focus inline-flex h-6 cursor-pointer items-center gap-[5px] rounded-full px-2.5 text-[12px] whitespace-nowrap text-ink-2 [&_svg]:size-3.5",
        "live:hover:text-ink data-[state=on]:bg-surface data-[state=on]:font-medium data-[state=on]:text-ink data-[state=on]:[--hx-shadow:var(--shadow-btn)]",
        "aria-disabled:cursor-default aria-disabled:opacity-45",
        className
      )}
      {...props}
    />
  )
  if (!(disabled && disabledReason)) return item
  // 单项禁用仍在 roving 序列里可聚焦，tooltip 写原因（A7）
  return (
    <Tooltip>
      <TooltipTrigger asChild>{item}</TooltipTrigger>
      <TooltipContent>{disabledReason}</TooltipContent>
    </Tooltip>
  )
}

export { Segmented, SegmentedItem }
