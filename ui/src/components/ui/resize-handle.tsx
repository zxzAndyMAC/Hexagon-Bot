// 分割线 ResizeHandle（components B8，workbench#resize；interaction §1「分割线行为」）。
// 宽 0 的定位容器：可见线 1px --line，命中区 9px，握柄 4×40 悬停 150ms 后淡入并跟随指针高度；
// 提示 rz-tip（标题 + 键位 + 「拖动以调整大小」），拖动时宽度标签 rz-badge 实时显示，触限变橙，吸附区提示改文案。
// 键盘（WCAG 2.2 拖动替代）：←→ 16px、Shift 64px、Home/End 到最小/最大、↵ 收展；role=separator 读宽度。
// 取值规则在 layout.ts（resolveDrag / keyTarget），本组件只管指针与外观。侧栏（票 02）首用，画布（03）、文件树、设置列表复用。
//
// 选型：没有用 react-resizable-panels（规范 B8 写的 shadcn Resizable）。侧栏的上下限、吸附宽度都是像素，
// 而 Resizable 以百分比布局、吸附成 56 宽图标列要另写；B8 的握柄、提示、宽度标签本来也要自绘。对账表票 02 记录。
import * as React from "react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { formatBinding } from "@/keymap"
import { keyTarget, resolveDrag, type DragResult, type Limits } from "@/layout"

/** 单击与双击的分辨窗口：单击要等这么久确认不是双击（单击侧栏分割线 = ⌘B，双击 = 还原默认宽）。 */
const DBL_MS = 250
/** 指针位移小于它算点击，不算拖动。 */
const CLICK_SLOP = 3
const HOVER_DELAY = 150

type ResizeHandleProps = {
  /** 可读名（`aria-label`），如「侧栏宽度」。 */
  label: string
  /** 当前宽度（`aria-valuenow`）。 */
  value: number
  limits: Limits
  /** 1 = 分割线右移变宽（侧栏），-1 = 左移变宽（画布）。 */
  direction?: 1 | -1
  /** 提示标题（「隐藏侧栏」）与它对应的绑定（规范形，如 `mod+B`）。 */
  tipTitle: string
  tipBinding?: string
  /** 提示副行追加（如「双击还原」）。 */
  tipExtra?: string
  /** 进入吸附区时提示与宽度标签改成这句（「松开以收起侧栏」）。 */
  snapLabel?: string
  /** 收起为图标列时：只响应单击（展开），不拖动（B8 禁用行）。 */
  collapsed?: boolean
  /** 第二个参数是拖动开始时的宽度（吸附收起时调用方据此保留展开宽度）。 */
  onDrag?: (r: DragResult, start: number) => void
  onCommit: (r: DragResult, start: number) => void
  onClick?: () => void
  onDoubleClick?: () => void
  /** ↵：收起或展开。 */
  onToggle?: () => void
  className?: string
}

function ResizeHandle({
  label,
  value,
  limits,
  direction = 1,
  tipTitle,
  tipBinding,
  tipExtra,
  snapLabel,
  collapsed = false,
  onDrag,
  onCommit,
  onClick,
  onDoubleClick,
  onToggle,
  className,
}: ResizeHandleProps) {
  const { t } = useTranslation()
  const [hover, setHover] = React.useState(false)
  const [gripY, setGripY] = React.useState<number | null>(null)
  const [drag, setDrag] = React.useState<DragResult | null>(null)
  const hoverTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const clickTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const root = React.useRef<HTMLDivElement>(null)

  React.useEffect(() => () => { clearTimeout(hoverTimer.current); clearTimeout(clickTimer.current) }, [])

  const click = () => {
    if (clickTimer.current) {
      clearTimeout(clickTimer.current)
      clickTimer.current = undefined
      onDoubleClick?.()
      return
    }
    clickTimer.current = setTimeout(() => { clickTimer.current = undefined; onClick?.() }, onDoubleClick ? DBL_MS : 0)
  }

  const pointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return
    e.preventDefault()
    const x0 = e.clientX
    const w0 = value
    let moved = false
    let last: DragResult | null = null
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - x0
      if (!moved && Math.abs(dx) < CLICK_SLOP) return
      if (collapsed) return
      moved = true
      last = resolveDrag(w0 + dx * direction, limits)
      setDrag(last)
      onDrag?.(last, w0)
    }
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
      window.removeEventListener("pointercancel", up)
      setDrag(null)
      if (moved && last) onCommit(last, w0)
      else click()
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
    window.addEventListener("pointercancel", up)
  }

  const keyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault()
      onToggle?.()
      return
    }
    if (collapsed) return
    const next = keyTarget(e.key, e.shiftKey, value, limits.min, limits.max, direction)
    if (next == null) return
    e.preventDefault()
    onCommit({ width: next, collapsed: false, edge: next === limits.min ? "min" : next === limits.max ? "max" : "none" }, value)
  }

  const hoverMove = (e: React.PointerEvent) => {
    const top = root.current?.getBoundingClientRect().top ?? 0
    setGripY(e.clientY - top)
  }
  const enter = (e: React.PointerEvent) => {
    hoverMove(e)
    clearTimeout(hoverTimer.current)
    hoverTimer.current = setTimeout(() => setHover(true), HOVER_DELAY)
  }
  const leave = () => {
    clearTimeout(hoverTimer.current)
    setHover(false)
  }

  const edge = drag?.edge
  const badge = drag
    ? edge === "snap" ? snapLabel ?? ""
      : edge === "min" ? t("layout.min", { n: drag.width })
      : edge === "max" ? t("layout.max", { n: drag.width })
      : t("layout.px", { n: drag.width })
    : null
  const tipLeft = direction === 1
  // 握柄跟随指针高度；键盘聚焦时（没有指针）居中
  const gripTop = gripY == null ? "calc(50% - 20px)" : `${Math.max(0, gripY - 20)}px`

  return (
    <div
      ref={root}
      data-slot="resize-handle"
      data-edge={edge ?? undefined}
      data-dragging={drag ? "" : undefined}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={Math.round(collapsed ? limits.collapsedWidth ?? value : value)}
      aria-valuemin={limits.min}
      aria-valuemax={limits.max}
      tabIndex={0}
      onKeyDown={keyDown}
      className={cn("group/rz relative z-[14] w-0 flex-none self-stretch outline-none", className)}
    >
      {/* 可见线：悬停 --line-strong，拖动 / 聚焦 2px accent，触限 --orange */}
      <span
        aria-hidden
        className={cn(
          "absolute inset-y-0 -left-px w-px bg-line transition-colors duration-150",
          "group-hover/rz:bg-line-strong group-focus-visible/rz:-left-px group-focus-visible/rz:w-[2px] group-focus-visible/rz:bg-primary",
          drag && "-left-px w-[2px] bg-primary",
          (edge === "min" || edge === "max") && "bg-orange",
        )}
      />
      <span
        aria-hidden
        data-slot="resize-grip"
        className={cn(
          "absolute -left-[2px] h-10 w-1 rounded-[4px] bg-ink-3 opacity-0 transition-opacity duration-150",
          "group-focus-visible/rz:opacity-100",
          (hover || drag) && "opacity-100",
          drag && "bg-primary",
        )}
        style={{ top: gripTop }}
      />
      <span
        aria-hidden
        className="absolute inset-y-0 -left-1 w-[9px] cursor-col-resize"
        onPointerDown={pointerDown}
        onPointerEnter={enter}
        onPointerMove={hoverMove}
        onPointerLeave={leave}
      />
      {hover && !drag && (
        <span
          aria-hidden
          data-slot="resize-tip"
          className={cn(
            "pointer-events-none absolute z-[25] whitespace-nowrap rounded-[10px] bg-tooltip px-2.5 py-2 text-[13px] text-tooltip-fg shadow-[var(--shadow-lg)]",
            tipLeft ? "left-[18px]" : "right-[18px]",
          )}
          style={{ top: `calc(${gripTop} - 4px)` }}
        >
          {tipTitle}
          {tipBinding && <span className="ml-2.5 font-mono text-[12px] opacity-55">{formatBinding(tipBinding)}</span>}
          <span className="mt-0.5 block text-[12px] opacity-65">
            {[t("layout.dragHint"), tipExtra].filter(Boolean).join(" · ")}
          </span>
        </span>
      )}
      {badge != null && (
        <span
          aria-hidden
          data-slot="resize-badge"
          className={cn(
            "pointer-events-none absolute top-2.5 z-[25] whitespace-nowrap rounded-[6px] bg-primary px-[7px] py-0.5 font-mono text-[11px] font-medium text-white",
            tipLeft ? "left-2" : "right-2",
          )}
        >
          {badge}
        </span>
      )}
      {/* 拖动期间整窗盖一层：文字不可选、iframe 预览不吃事件（interaction §1） */}
      {drag && <span aria-hidden className="fixed inset-0 z-[15] cursor-col-resize select-none" />}
    </div>
  )
}

export { ResizeHandle }
export type { ResizeHandleProps }
