// 图标按钮 IconButton（components A2）：28×28（sm 24×24）方形，圆角 --r-sm（sm 6），--ink-2。
// 必须有可读名；tooltip = 名称 + 当前绑定（ADR 0051：传 action 时从 keymap 读用户当前绑定，不写死）。
// on = 开关已开或当前页（aria-pressed）。禁用走 aria-disabled，仍可聚焦、tooltip 写原因。
import * as React from "react"
import { cva } from "class-variance-authority"
import { cn } from "@/lib/utils"
import { bindingFor, type ActionId } from "@/keymap"
import { ShortcutKbd } from "@/components/ui/kbd"
import { Spinner } from "@/components/ui/spinner"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { guardInert } from "@/lib/inert"

const iconButtonVariants = cva(
  "hx-press hx-focus inline-grid shrink-0 cursor-pointer place-items-center text-ink-2 live:hover:bg-hover live:hover:text-ink aria-pressed:bg-hover-2 aria-pressed:text-ink aria-disabled:cursor-default aria-disabled:opacity-45 [&_svg]:pointer-events-none",
  {
    variants: {
      size: {
        md: "size-7 rounded-[var(--r-sm)] [&_svg:not([class*='size-'])]:size-4",
        sm: "size-6 rounded-[var(--r-xs)] [&_svg:not([class*='size-'])]:size-3.5",
      },
    },
    defaultVariants: { size: "md" },
  }
)

type IconButtonProps = Omit<React.ComponentProps<"button">, "aria-label"> & {
  /** 可读名（aria-label + tooltip 首行）。 */
  label: string
  /** 绑定的 keymap 动作：tooltip 显示当前绑定。 */
  action?: ActionId
  /** 禁用时 tooltip 显示的原因（替代名称后的绑定）。 */
  disabledReason?: string
  size?: "md" | "sm"
  on?: boolean
  loading?: boolean
}

function IconButton({
  label,
  action,
  disabledReason,
  size = "md",
  on,
  loading = false,
  disabled,
  onClick,
  className,
  children,
  type = "button",
  ...props
}: IconButtonProps) {
  const inert = disabled || loading
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          data-slot="icon-button"
          type={type}
          aria-label={label}
          aria-pressed={on === undefined ? undefined : on}
          aria-disabled={disabled || undefined}
          aria-busy={loading || undefined}
          className={cn(iconButtonVariants({ size }), className)}
          onClick={guardInert(inert, onClick)}
          {...props}
        >
          {loading ? <Spinner /> : children}
        </button>
      </TooltipTrigger>
      <TooltipContent>
        <span>{disabled && disabledReason ? disabledReason : label}</span>
        {action && !(disabled && disabledReason) && <ShortcutKbd binding={bindingFor(action)} />}
      </TooltipContent>
    </Tooltip>
  )
}

export { IconButton, iconButtonVariants }
