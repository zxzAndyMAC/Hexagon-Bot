// 复选框 Checkbox（components A11）：16×16、圆角 5；未选 1.5px --line-strong 内环 + --surface 底；
// 选中 --accent 底白勾。悬停未选描边转 --ink-3；必勾未勾（aria-invalid）描边 --red。
import * as React from "react"
import { Checkbox as CheckboxPrimitive } from "radix-ui"
import { Check } from "lucide-react"
import { cn } from "@/lib/utils"
import { guardInert } from "@/lib/inert"

function Checkbox({ className, disabled, onClick, ...props }: React.ComponentProps<typeof CheckboxPrimitive.Root>) {
  return (
    <CheckboxPrimitive.Root
      data-slot="checkbox"
      aria-disabled={disabled || undefined}
      onClick={guardInert(disabled, onClick)}
      className={cn(
        "hx-press hx-focus grid size-4 shrink-0 cursor-pointer place-items-center rounded-[5px] bg-surface text-white",
        "[--hx-shadow:inset_0_0_0_1.5px_var(--line-strong)] live:hover:data-[state=unchecked]:[--hx-shadow:inset_0_0_0_1.5px_var(--ink-3)]",
        "aria-invalid:data-[state=unchecked]:[--hx-shadow:inset_0_0_0_1.5px_var(--red)]!",
        "data-[state=checked]:bg-primary data-[state=checked]:[--hx-shadow:0_0_#0000]",
        "aria-disabled:cursor-default aria-disabled:opacity-45",
        className
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator data-slot="checkbox-indicator" className="grid place-items-center">
        <Check className="size-3" strokeWidth={2.5} />
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  )
}

export { Checkbox }
