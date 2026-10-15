// Toast（components A18，feedback#toast）。shadcn Sonner 改写为项目形制：
// 38 高胶囊、--tooltip-bg / --tooltip-fg、13px、--shadow-lg；前置 16 图标（circleCheck 绿 / undo / circleX 红）；
// 至多一个文字动作。浮在输入条上方居中，约 3.5s，悬停暂停（Sonner 自带），不抢焦点。
// 错误行 role="alert"（A18 错误态）；其余 role="status"（容器本身 aria-live=polite）。
// 未用 shadcn 生成件的 next-themes：主题由 html[data-theme] 的 CSS 变量决定，Sonner 只当计时与堆叠容器（unstyled）。
import { Toaster as Sonner, type ToasterProps } from "sonner"
import { cn } from "@/lib/utils"
import { Icon } from "@/components/Icon"

/** 输入条上方：底栏输入条约 64 高 + 8 间距（沿用旧 .toast-host 的 bottom: 72px）。 */
const OFFSET = 72
export const TOAST_DURATION_MS = 3500

function Toaster(props: ToasterProps) {
  return (
    <Sonner
      position="bottom-center"
      offset={OFFSET}
      duration={TOAST_DURATION_MS}
      gap={8}
      visibleToasts={3}
      toastOptions={{ unstyled: true, className: "flex w-full justify-center" }}
      {...props}
    />
  )
}

export type ToastTone = "info" | "ok" | "err"

/** 胶囊本体：给 toast.custom 渲染，也给静态比对用。 */
function ToastPill({ tone, icon, text, code, action, onAction, className }: {
  tone: ToastTone
  icon?: "undo"
  text: string
  code?: string
  action?: string
  onAction?: () => void
  className?: string
}) {
  const glyph = icon === "undo"
    ? <Icon name="undo" size={16} />
    : tone === "ok" ? <Icon name="circle-check" size={16} className="text-green" />
    : tone === "err" ? <Icon name="circle-x" size={16} className="text-red" />
    : null
  return (
    <div
      data-slot="toast"
      data-tone={tone}
      role={tone === "err" ? "alert" : "status"}
      className={cn(
        "pointer-events-auto inline-flex min-h-[38px] max-w-[min(560px,80vw)] animate-hx-jumpin items-center gap-2 rounded-full bg-tooltip py-2 pr-3.5 pl-3 text-[13px] leading-[18px] text-tooltip-fg shadow-[var(--shadow-lg)]",
        className,
      )}
    >
      {glyph && <span className="flex shrink-0">{glyph}</span>}
      <span className="min-w-0 whitespace-pre-line">
        {text}
        {code && <span className="ml-1 font-mono text-[12px] opacity-60">（{code}）</span>}
      </span>
      {action && (
        <button
          type="button"
          data-toast-action
          className="hx-focus ml-3 shrink-0 cursor-pointer rounded-[4px] text-[12px] opacity-60 live:hover:underline live:hover:opacity-100"
          onClick={onAction}
        >
          {action}
        </button>
      )}
    </div>
  )
}

export { Toaster, ToastPill }
