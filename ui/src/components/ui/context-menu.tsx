// 右键菜单 ContextMenu（components A16b，2026-10-17 负责人裁决；owner-followups-1015 #05）。
// 外壳与行完全同 A16（popSurface / menuItemBase / 分组标题 / 分隔），在指针处打开。
// - 行右侧灰字 = 当前绑定（传 action 时从 keymap 读，同 tooltip 规则），未绑定不显示；
//   禁用行右侧改写原因（「右侧没有页签」）。种类不适用的项由调用方不渲染。
// - 触发：右键；macOS ctrl+点击由系统发 contextmenu；焦点在触发器（或其内）上按 Shift+F10 / Menu 键，
//   菜单位置取触发器矩形左下角（WKWebView 不会把这两个键翻成 contextmenu，只能自己派发）。
// - 右键不改选中：触发器只拿 data-state=open 画焦点环，不点击、不激活；关闭后焦点回触发处
//   （radix 默认把焦点还给打开前的 activeElement——右键前焦点常在输入条，回那里等于丢掉位置）。
// - 禁区：裁决动作（允许 / 拒绝 / 执行这一次 / 选项）永不放进来——误点代价是一次未审副作用，偏向 fail-closed。
import * as React from "react"
import { ContextMenu as MenuPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"
import { useFloatingZ } from "@/components/ui/layer"
import { menuItemBase, popSurface } from "@/components/ui/dropdown-menu"
import { bindingFor, formatBinding, type ActionId } from "@/keymap"

const TriggerRef = React.createContext<React.RefObject<HTMLElement | null> | null>(null)

/** 触发器内可聚焦的落点：自身可聚焦则自身，否则第一个 tab / 按钮 / tabindex 后代。 */
function focusTarget(el: HTMLElement): HTMLElement {
  if (el.matches('button, a[href], [tabindex]')) return el
  return el.querySelector<HTMLElement>('[role="tab"], [role="treeitem"], button, a[href], [tabindex]') ?? el
}

function ContextMenu({ children, ...props }: React.ComponentProps<typeof MenuPrimitive.Root>) {
  const trigger = React.useRef<HTMLElement | null>(null)
  return (
    <TriggerRef.Provider value={trigger}>
      <MenuPrimitive.Root {...props}>{children}</MenuPrimitive.Root>
    </TriggerRef.Provider>
  )
}

/** 键盘打开：Shift+F10 或 Menu 键（KeyboardEvent.key = "ContextMenu"）。 */
export function isContextMenuKey(e: React.KeyboardEvent | KeyboardEvent): boolean {
  return e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey)
}

function ContextMenuTrigger({ className, onKeyDown, onContextMenu, ...props }: React.ComponentProps<typeof MenuPrimitive.Trigger>) {
  const trigger = React.useContext(TriggerRef)
  return (
    <MenuPrimitive.Trigger
      data-slot="context-menu-trigger"
      // 右键期间的焦点环标记（「不改选中」：只画环，不选中）；hx-focus 的环同 token。
      className={cn("data-[state=open]:shadow-[var(--hx-shadow),var(--focus-ring)]", className)}
      onContextMenu={(e) => {
        if (trigger) trigger.current = e.currentTarget as HTMLElement
        onContextMenu?.(e)
      }}
      onKeyDown={(e) => {
        onKeyDown?.(e)
        if (e.defaultPrevented || !isContextMenuKey(e)) return
        e.preventDefault()
        const host = e.currentTarget as HTMLElement
        const r = host.getBoundingClientRect()
        host.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left, clientY: r.bottom }))
      }}
      {...props}
    />
  )
}

function ContextMenuContent({ className, onCloseAutoFocus, ...props }: React.ComponentProps<typeof MenuPrimitive.Content>) {
  const z = useFloatingZ("pop")
  const trigger = React.useContext(TriggerRef)
  return (
    <MenuPrimitive.Portal>
      <MenuPrimitive.Content
        data-slot="context-menu-content"
        className={cn(popSurface, z, "max-h-(--radix-context-menu-content-available-height)", className)}
        onCloseAutoFocus={(e) => {
          onCloseAutoFocus?.(e)
          if (e.defaultPrevented) return
          const el = trigger?.current
          if (!el?.isConnected) return
          e.preventDefault()
          focusTarget(el).focus()
        }}
        {...props}
      />
    </MenuPrimitive.Portal>
  )
}

function ContextMenuItem({
  className,
  icon,
  action,
  reason,
  disabled,
  children,
  ...props
}: Omit<React.ComponentProps<typeof MenuPrimitive.Item>, "disabled"> & {
  icon?: React.ReactNode
  /** 绑定的 keymap 动作：行右侧灰字显示当前绑定（未绑定不显示）。 */
  action?: ActionId
  /** 此刻做不了的原因；给了就禁用，右侧写原因代替绑定。 */
  reason?: string
  disabled?: boolean
}) {
  const binding = action ? bindingFor(action) : ""
  const off = disabled || !!reason
  const hint = reason ?? (binding ? formatBinding(binding) : null)
  return (
    <MenuPrimitive.Item data-slot="context-menu-item" disabled={off} className={cn(menuItemBase, "h-8", className)} {...props}>
      {icon}
      {children}
      {hint && <span data-ctx-hint className="ml-auto pl-3 text-[12px] text-ink-3">{hint}</span>}
    </MenuPrimitive.Item>
  )
}

function ContextMenuLabel({ className, ...props }: React.ComponentProps<typeof MenuPrimitive.Label>) {
  return <MenuPrimitive.Label data-slot="context-menu-label" className={cn("px-[9px] pt-2 pb-1 text-[11.5px] text-ink-3", className)} {...props} />
}

function ContextMenuSeparator({ className, ...props }: React.ComponentProps<typeof MenuPrimitive.Separator>) {
  return <MenuPrimitive.Separator data-slot="context-menu-separator" className={cn("mx-0.5 my-1 h-px bg-line", className)} {...props} />
}

export { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuLabel, ContextMenuSeparator, ContextMenuTrigger }
