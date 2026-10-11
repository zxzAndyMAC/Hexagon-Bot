// 开关 Switch（components A10）：30×18 轨，14 白滑块 + 0 1px 2px 阴影；关态轨 --hover-2 + --line 内环；
// 开态 --accent、滑块右移 12（200ms --ease）。悬停关态内环转 --line-strong；按下滑块 scale(.98)。
// 加载 = 宿主确认前不乐观翻转：保持原位、旁置 ring（如托管开启）；错误回原位由调用方旁置红字。
import * as React from "react"
import { Switch as SwitchPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"
import { Spinner } from "@/components/ui/spinner"
import { guardInert } from "@/lib/inert"

function Switch({
  className,
  loading = false,
  disabled,
  onClick,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root> & { loading?: boolean }) {
  const root = (
    <SwitchPrimitive.Root
      data-slot="switch"
      aria-disabled={disabled || undefined}
      aria-busy={loading || undefined}
      onClick={guardInert(disabled || loading, onClick)}
      className={cn(
        "group/switch hx-focus relative inline-flex h-[18px] w-[30px] shrink-0 cursor-pointer items-center rounded-full transition-[background-color] duration-150 ease-hx",
        "bg-hover-2 [--hx-shadow:inset_0_0_0_1px_var(--line)] live:hover:data-[state=unchecked]:[--hx-shadow:inset_0_0_0_1px_var(--line-strong)]",
        "data-[state=checked]:bg-primary data-[state=checked]:[--hx-shadow:0_0_#0000]",
        "aria-disabled:cursor-default aria-disabled:opacity-45 aria-busy:cursor-default",
        className
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block size-3.5 translate-x-0.5 rounded-full bg-white shadow-[0_1px_2px_oklch(0%_0_0/.25)] transition-transform duration-200 ease-hx group-active/switch:scale-[.98] data-[state=checked]:translate-x-[14px] data-[state=checked]:group-active/switch:scale-[.98]"
      />
    </SwitchPrimitive.Root>
  )
  if (!loading) return root
  return (
    <span className="inline-flex items-center gap-2">
      {root}
      <Spinner size={12} />
    </span>
  )
}

export { Switch }
