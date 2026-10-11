// 确认类模态 AlertDialog（components A17：确认层 feedback#confirm、广泛访问确认、托管开启说明、离开未保存）。
// 外壳与 Dialog 同（圆角 16、遮罩 --scrim、jumpin、背景裁决快捷键拦截）。
// 首焦：破坏性/放权类 = 取消（Radix AlertDialog 默认聚焦 Cancel）；安全类由调用方 autoFocus 主按钮。
// busy（提交中）：所有按钮禁用、Esc 无效、主按钮环形加载（aria-busy）。
import * as React from "react"
import { AlertDialog as AlertPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"
import { Button, type ButtonProps } from "@/components/ui/button"
import { Kbd } from "@/components/ui/kbd"
import { modalClass, overlayClass, useBackgroundDecisionGuard } from "@/components/ui/dialog"
import { guardBusy } from "@/lib/inert"

// 提交中：所有按钮禁用（A17），主按钮另行由调用方给 loading
const Busy = React.createContext(false)

const AlertDialog = AlertPrimitive.Root
const AlertDialogTrigger = AlertPrimitive.Trigger

function AlertDialogContent({
  className,
  busy = false,
  onEscapeKeyDown,
  children,
  ...props
}: React.ComponentProps<typeof AlertPrimitive.Content> & { busy?: boolean }) {
  useBackgroundDecisionGuard()
  return (
    <AlertPrimitive.Portal>
      <AlertPrimitive.Overlay data-slot="alert-dialog-overlay" className={overlayClass} />
      <AlertPrimitive.Content
        data-slot="alert-dialog-content"
        aria-busy={busy || undefined}
        className={cn(modalClass, className)}
        onEscapeKeyDown={guardBusy(busy, onEscapeKeyDown)}
        {...props}
      >
        <Busy.Provider value={busy}>{children}</Busy.Provider>
      </AlertPrimitive.Content>
    </AlertPrimitive.Portal>
  )
}

function AlertDialogTitle({ className, ...props }: React.ComponentProps<typeof AlertPrimitive.Title>) {
  return <AlertPrimitive.Title data-slot="alert-dialog-title" className={cn("text-[15px] font-semibold", className)} {...props} />
}

function AlertDialogDescription({ className, ...props }: React.ComponentProps<typeof AlertPrimitive.Description>) {
  return <AlertPrimitive.Description data-slot="alert-dialog-description" className={cn("text-[13px] text-ink-2", className)} {...props} />
}

/** 「取消 Esc」：带 Esc 键位提示。 */
function AlertDialogCancel({ children, disabled, ...props }: ButtonProps) {
  const busy = React.useContext(Busy)
  return (
    <AlertPrimitive.Cancel asChild>
      <Button variant="default" disabled={disabled || busy} {...props}>
        {children}
        <Kbd>Esc</Kbd>
      </Button>
    </AlertPrimitive.Cancel>
  )
}

/** 主按钮：不自动关闭（异步提交完成后由调用方关），以便失败时模态不关、显示错误。 */
function AlertDialogAction({ variant = "primary", loading, disabled, ...props }: ButtonProps) {
  const busy = React.useContext(Busy)
  return <Button variant={variant} loading={loading} disabled={disabled || (busy && !loading)} {...props} />
}

export {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogTitle,
  AlertDialogTrigger,
}
export { DialogError as AlertDialogError, DialogFooter as AlertDialogFooter, DialogHeader as AlertDialogHeader, DialogIcon as AlertDialogIcon } from "@/components/ui/dialog"
