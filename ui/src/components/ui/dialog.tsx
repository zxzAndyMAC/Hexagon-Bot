// 模态 Dialog（components A17，查看类：图片放大等）。外壳：居中、圆角 16、--surface、--shadow-overlay，
// 遮罩 --scrim（z 15）、模态 z 30，打开时 jumpin（foundations §6）。
// 打开期间在捕获阶段拦掉背景裁决快捷键（modalDecisionGuard，与旧原生 dialog 共用同一张动作表）；
// 焦点困在模态内由 Radix 负责。busy（提交中）时 Esc 与点遮罩都不关。
import * as React from "react"
import { Dialog as DialogPrimitive } from "radix-ui"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"
import { blockBackgroundDecision } from "@/modalDecisionGuard"
import { guardBusy } from "@/lib/inert"
import { InModalLayer } from "@/components/ui/layer"

const Dialog = DialogPrimitive.Root
const DialogTrigger = DialogPrimitive.Trigger
const DialogClose = DialogPrimitive.Close

/** 模态打开期间吞掉背景裁决快捷键（AGENTS.md 裁决回路：modalScope 拦截）。 */
function useBackgroundDecisionGuard() {
  React.useEffect(() => {
    window.addEventListener("keydown", blockBackgroundDecision, true)
    return () => window.removeEventListener("keydown", blockBackgroundDecision, true)
  }, [])
}

const overlayClass = "fixed inset-0 z-[15] bg-scrim data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
const modalClass =
  "fixed top-1/2 left-1/2 z-30 flex w-[min(440px,calc(100vw-48px))] -translate-x-1/2 -translate-y-1/2 flex-col gap-3 rounded-[16px] bg-surface p-5 text-ink shadow-[var(--shadow-overlay)] outline-none data-open:animate-hx-jumpin"

function DialogContent({
  className,
  busy = false,
  container,
  children,
  onEscapeKeyDown,
  onPointerDownOutside,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Content> & {
  busy?: boolean
  /** Portal 挂载点（默认 body）；标本页把模态钉在格子里做静态比对时用。 */
  container?: HTMLElement | null
}) {
  useBackgroundDecisionGuard()
  return (
    <DialogPrimitive.Portal container={container}>
      <DialogPrimitive.Overlay data-slot="dialog-overlay" className={overlayClass} />
      <DialogPrimitive.Content
        data-slot="dialog-content"
        aria-busy={busy || undefined}
        className={cn(modalClass, className)}
        onEscapeKeyDown={guardBusy(busy, onEscapeKeyDown)}
        onPointerDownOutside={guardBusy(busy, onPointerDownOutside)}
        {...props}
      >
        <InModalLayer>{children}</InModalLayer>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  )
}

/** 头部 28 图标块（ic 变体，decisions#dock 同形）。 */
const dialogIconVariants = cva("grid size-7 shrink-0 place-items-center rounded-[8px] [&_svg]:size-4", {
  variants: {
    tone: {
      warn: "bg-orange-tint text-orange",
      err: "bg-red-tint text-red",
      flag: "bg-violet-tint text-violet",
      info: "bg-accent-tint text-accent-ink",
      ok: "bg-green-tint text-green",
      mute: "bg-canvas text-ink-2",
    },
  },
  defaultVariants: { tone: "mute" },
})

function DialogIcon({ className, tone, ...props }: React.ComponentProps<"span"> & VariantProps<typeof dialogIconVariants>) {
  return <span data-slot="dialog-icon" aria-hidden className={cn(dialogIconVariants({ tone }), className)} {...props} />
}

function DialogHeader({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="dialog-header" className={cn("flex items-center gap-2.5", className)} {...props} />
}

/** 标题 15/600。 */
function DialogTitle({ className, ...props }: React.ComponentProps<typeof DialogPrimitive.Title>) {
  return <DialogPrimitive.Title data-slot="dialog-title" className={cn("text-[15px] font-semibold", className)} {...props} />
}

/** 正文 13 --ink-2。 */
function DialogDescription({ className, ...props }: React.ComponentProps<typeof DialogPrimitive.Description>) {
  return <DialogPrimitive.Description data-slot="dialog-description" className={cn("text-[13px] text-ink-2", className)} {...props} />
}

/** 底栏上方 role=alert 红字写原因，模态不关。 */
function DialogError({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="dialog-error" role="alert" className={cn("text-[12.5px] text-red", className)} {...props} />
}

/** 底栏右对齐「取消 Esc」+ 主按钮。 */
function DialogFooter({ className, ...props }: React.ComponentProps<"div">) {
  return <div data-slot="dialog-footer" className={cn("mt-1 flex items-center justify-end gap-2", className)} {...props} />
}

export {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogError,
  DialogFooter,
  DialogHeader,
  DialogIcon,
  DialogTitle,
  DialogTrigger,
  modalClass,
  overlayClass,
  useBackgroundDecisionGuard,
}
