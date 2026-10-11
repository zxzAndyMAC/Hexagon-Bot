// 按钮 Button（components A1）。shadcn variant 映射：default→outline 形、primary→default、
// ink→新增、danger→destructive、ghost→ghost、quiet→secondary。尺寸 xs 24 / sm 28 / md 32 / lg 38。
// 禁用走 aria-disabled 而不是原生 disabled：规范 §0「禁用仍可聚焦以读出原因（tooltip 写原因）」，
// 原生 disabled 会把按钮移出 Tab 序列、tooltip 也触发不了。
import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"
import { ChevronDown } from "lucide-react"
import { cn } from "@/lib/utils"
import { Spinner } from "@/components/ui/spinner"
import { guardInert } from "@/lib/inert"

const buttonVariants = cva(
  "hx-press hx-focus inline-flex shrink-0 cursor-pointer select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-full font-medium aria-disabled:cursor-default aria-disabled:opacity-45 aria-busy:cursor-default [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: "bg-surface text-ink [--hx-shadow:var(--shadow-btn)] live:hover:bg-hover",
        primary:
          "bg-primary text-white [--hx-shadow:0_1px_2px_oklch(0%_0_0/.12),inset_0_1px_0_oklch(100%_0_0/.14)] live:hover:opacity-92",
        ink: "bg-ink text-page live:hover:opacity-92",
        danger: "bg-red text-white live:hover:opacity-92",
        ghost: "bg-transparent text-ink-2 live:hover:bg-hover live:hover:text-ink",
        quiet: "bg-canvas text-ink-2 live:hover:bg-hover",
      },
      size: {
        xs: "h-6 gap-1 px-[9px] text-[12px] [&_svg:not([class*='size-'])]:size-3",
        sm: "h-7 px-[11px] text-[12.5px] [&_svg:not([class*='size-'])]:size-3.5",
        md: "h-8 px-3.5 text-[13px]",
        lg: "h-[38px] px-[18px] text-[14px]",
      },
    },
    defaultVariants: { variant: "default", size: "md" },
  }
)

type ButtonProps = React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
    /** 环形加载：文字前换 14px ring，控件不可再点（文字改进行时由调用方给）。 */
    loading?: boolean
  }

const SOLID = new Set(["primary", "ink", "danger"])

function Button({
  className,
  variant = "default",
  size = "md",
  asChild = false,
  loading = false,
  disabled,
  onClick,
  children,
  type = "button",
  ...props
}: ButtonProps) {
  const Comp = asChild ? Slot.Root : "button"
  const inert = disabled || loading
  return (
    <Comp
      data-slot="button"
      data-variant={variant}
      data-size={size}
      type={asChild ? undefined : type}
      aria-disabled={disabled || undefined}
      aria-busy={loading || undefined}
      className={cn(buttonVariants({ variant, size, className }))}
      onClick={guardInert(inert, onClick)}
      {...props}
    >
      {asChild ? children : (
        <>
          {loading && <Spinner tone={SOLID.has(variant ?? "") ? "solid" : "default"} />}
          {children}
        </>
      )}
    </Comp>
  )
}

/** split 变体（A1）：primary 右侧 ▾ 分段，左边线 oklch(100% 0 0 / .3)。▾ 是独立按钮，由调用方包 DropdownMenuTrigger。 */
function SplitButton({
  className,
  size = "md",
  menuLabel,
  menuTrigger,
  children,
  ...props
}: Omit<ButtonProps, "variant"> & {
  menuLabel: string
  /** 把 ▾ 包进 DropdownMenuTrigger 的渲染函数；不传则 ▾ 只是按钮。 */
  menuTrigger?: (caret: React.ReactElement) => React.ReactNode
}) {
  const caret = (
    <Button
      variant="primary"
      size={size}
      aria-label={menuLabel}
      disabled={props.disabled || props.loading}
      className="rounded-l-none pr-2.5 pl-1.5 before:mr-1.5 before:h-[18px] before:border-l before:border-[oklch(100%_0_0/.3)] before:content-['']"
    >
      <ChevronDown />
    </Button>
  )
  return (
    <span data-slot="split-button" className={cn("inline-flex", className)}>
      <Button variant="primary" size={size} className="rounded-r-none pr-2" {...props}>
        {children}
      </Button>
      {menuTrigger ? menuTrigger(caret) : caret}
    </span>
  )
}

export { Button, SplitButton, buttonVariants }
export type { ButtonProps }
