// 菜单与弹层 Menu（components A16）：pop 圆角 12、--surface、--shadow-overlay、padding 5、最小宽 220；
// 行 mi 32 高、padding 0 9、圆角 7、13px，图标 + 名 + 右侧灰描述（12px ink-3）或 ✓；高行两行（粗名 + 灰描述）；
// 分组标题 11.5 ink-3；分隔 1px --line。悬停与键盘高亮同为 --hover 底（Radix data-highlighted）。
// 禁用行描述写原因；错误行 --red 文案 + 重试由调用方放 MenuItem。z 序 20（foundations §4）。
import * as React from "react"
import { DropdownMenu as MenuPrimitive } from "radix-ui"
import { Check } from "lucide-react"
import { cn } from "@/lib/utils"
import { useFloatingZ } from "@/components/ui/layer"

const DropdownMenu = MenuPrimitive.Root
const DropdownMenuTrigger = MenuPrimitive.Trigger
const DropdownMenuGroup = MenuPrimitive.Group
const DropdownMenuRadioGroup = MenuPrimitive.RadioGroup

/** 弹层外壳（菜单与 Popover 共用）。 */
const popSurface =
  "z-20 min-w-[220px] overflow-y-auto rounded-[12px] bg-surface p-[5px] text-ink shadow-[var(--shadow-overlay)] data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"

function DropdownMenuContent({ className, sideOffset = 6, ...props }: React.ComponentProps<typeof MenuPrimitive.Content>) {
  const z = useFloatingZ("pop")
  return (
    <MenuPrimitive.Portal>
      <MenuPrimitive.Content
        data-slot="dropdown-menu-content"
        sideOffset={sideOffset}
        className={cn(popSurface, z, "max-h-(--radix-dropdown-menu-content-available-height)", className)}
        {...props}
      />
    </MenuPrimitive.Portal>
  )
}

const itemBase =
  "hx-press relative flex cursor-pointer items-center gap-[9px] rounded-[7px] px-[9px] text-[13px] outline-none select-none data-highlighted:bg-hover data-disabled:cursor-default data-disabled:opacity-45 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-ink-2"

function DropdownMenuItem({
  className,
  icon,
  description,
  tall = false,
  tone,
  children,
  ...props
}: React.ComponentProps<typeof MenuPrimitive.Item> & {
  icon?: React.ReactNode
  /** 右侧灰描述；tall 时为第二行。禁用行写原因。 */
  description?: React.ReactNode
  tall?: boolean
  tone?: "danger"
}) {
  return (
    <MenuPrimitive.Item
      data-slot="dropdown-menu-item"
      className={cn(itemBase, tall ? "items-start py-2" : "h-8", tone === "danger" && "text-red-ink [&_svg]:text-red", className)}
      {...props}
    >
      {icon}
      {tall ? (
        <span className="flex min-w-0 flex-col">
          <span className="font-medium">{children}</span>
          {description && <span className="text-[12px] text-ink-3">{description}</span>}
        </span>
      ) : (
        <>
          {children}
          {description && <span className="ml-auto pl-3 text-[12px] text-ink-3">{description}</span>}
        </>
      )}
    </MenuPrimitive.Item>
  )
}

function DropdownMenuRadioItem({
  className,
  description,
  children,
  ...props
}: React.ComponentProps<typeof MenuPrimitive.RadioItem> & { description?: React.ReactNode }) {
  return (
    <MenuPrimitive.RadioItem data-slot="dropdown-menu-radio-item" className={cn(itemBase, "h-8", className)} {...props}>
      {children}
      {description && <span className="ml-auto pl-3 text-[12px] text-ink-3">{description}</span>}
      <MenuPrimitive.ItemIndicator className={cn(!description && "ml-auto", "inline-flex")}>
        <Check className="text-ink!" />
      </MenuPrimitive.ItemIndicator>
    </MenuPrimitive.RadioItem>
  )
}

function DropdownMenuLabel({ className, ...props }: React.ComponentProps<typeof MenuPrimitive.Label>) {
  return <MenuPrimitive.Label data-slot="dropdown-menu-label" className={cn("px-[9px] pt-2 pb-1 text-[11.5px] text-ink-3", className)} {...props} />
}

function DropdownMenuSeparator({ className, ...props }: React.ComponentProps<typeof MenuPrimitive.Separator>) {
  return <MenuPrimitive.Separator data-slot="dropdown-menu-separator" className={cn("mx-0.5 my-1 h-px bg-line", className)} {...props} />
}

/** 加载：列表首屏 3 行骨架（A16）。 */
function DropdownMenuSkeleton({ label }: { label: string }) {
  return (
    <div data-slot="dropdown-menu-skeleton" role="status" aria-label={label} className="flex flex-col gap-1 p-1">
      {[0, 1, 2].map((i) => (
        <span key={i} className="h-6 animate-hx-skl rounded-[7px] bg-[linear-gradient(90deg,var(--canvas)_25%,var(--hover-2)_50%,var(--canvas)_75%)] bg-[length:200%_100%]" />
      ))}
    </div>
  )
}

/** 错误：单行 --red 文案 + 「重试」（A16）。 */
function DropdownMenuError({ message, retryLabel, onRetry }: { message: string; retryLabel: string; onRetry: () => void }) {
  return (
    <div data-slot="dropdown-menu-error" role="alert" className="flex h-8 items-center gap-2 px-[9px] text-[13px] text-red-ink">
      <span className="truncate">{message}</span>
      <MenuPrimitive.Item onSelect={(e) => { e.preventDefault(); onRetry() }} className="ml-auto cursor-pointer rounded-[6px] px-1.5 text-[12.5px] text-ink outline-none data-highlighted:bg-hover">
        {retryLabel}
      </MenuPrimitive.Item>
    </div>
  )
}

/** 底部提示（「↑↓ 选择 · ↵ 插入 · Esc 关闭」）11.5 ink-3。 */
function DropdownMenuFooter({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="dropdown-menu-footer" className={cn("px-[9px] pt-1.5 pb-1 text-[11.5px] text-ink-3", className)} {...props} />
}

export {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuError,
  DropdownMenuFooter,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSkeleton,
  DropdownMenuTrigger,
  popSurface,
}
