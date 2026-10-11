// 单选 Radio（components A11）：16×16 圆；未选 1.5px --line-strong 内环；选中 inset 0 0 0 5px --accent。
// 悬停未选描边转 --ink-3；↑↓/←→ 在组内移动（Radix roving）。
import * as React from "react"
import { RadioGroup as RadioGroupPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"
import { guardInert } from "@/lib/inert"

// 组级禁用同样不下发原生 disabled（见 lib/inert），经 context 交给每个单选项
const GroupDisabled = React.createContext(false)

function RadioGroup({ className, disabled = false, ...props }: React.ComponentProps<typeof RadioGroupPrimitive.Root>) {
  return (
    <GroupDisabled.Provider value={disabled}>
      <RadioGroupPrimitive.Root data-slot="radio-group" aria-disabled={disabled || undefined} className={cn("grid gap-2", className)} {...props} />
    </GroupDisabled.Provider>
  )
}

function RadioGroupItem({ className, disabled: own, onClick, ...props }: React.ComponentProps<typeof RadioGroupPrimitive.Item>) {
  const disabled = React.useContext(GroupDisabled) || own
  return (
    <RadioGroupPrimitive.Item
      data-slot="radio-group-item"
      aria-disabled={disabled || undefined}
      onClick={guardInert(disabled, onClick)}
      className={cn(
        "hx-press hx-focus size-4 shrink-0 cursor-pointer rounded-full bg-surface",
        "[--hx-shadow:inset_0_0_0_1.5px_var(--line-strong)] live:hover:data-[state=unchecked]:[--hx-shadow:inset_0_0_0_1.5px_var(--ink-3)]",
        "aria-invalid:data-[state=unchecked]:[--hx-shadow:inset_0_0_0_1.5px_var(--red)]!",
        "data-[state=checked]:[--hx-shadow:inset_0_0_0_5px_var(--accent)]",
        "aria-disabled:cursor-default aria-disabled:opacity-45",
        className
      )}
      {...props}
    />
  )
}

export { RadioGroup, RadioGroupItem }
