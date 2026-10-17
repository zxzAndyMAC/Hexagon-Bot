// 行内错误 InlineError（components C5，feedback#errors / feedback#loading）。
// 块形：--red-tint 底圆角 10 padding 10 12；行形：无底，居中一行「<什么>失败 · 重试」。
// 头：circleX 16 红 + 标题 13/500「<什么>失败：<原因>（<码>）」+ 等宽元信息 12 ink-3；
// 说明 12.5 ink-2；补救按钮只给有对应补救页面的（临时性错误给「重试」）；不可用时不渲染按钮，只写原因。
// 重试中：按钮环形加载，标题保留。role="alert"。
import type { ReactNode } from "react"
import { cn } from "@/lib/utils"
import { Icon } from "@/components/Icon"
import { Button } from "@/components/ui/button"

function InlineError({ title, code, meta, desc, actions, onRetry, retryLabel, retryTitle, retrying, variant = "block", className, ...rest }: {
  title: ReactNode
  code?: string
  meta?: ReactNode
  desc?: ReactNode
  /** 补救按钮（只给有对应补救页面的）。 */
  actions?: ReactNode
  /** 临时性错误的「重试」。 */
  onRetry?: () => void
  retryLabel?: string
  /** 重试按钮 tooltip（带当前键位绑定）。 */
  retryTitle?: string
  retrying?: boolean
  variant?: "block" | "row"
  className?: string
} & Record<`data-${string}`, string | boolean | undefined>) {
  const retry = onRetry && retryLabel
    ? <Button size="xs" title={retryTitle} loading={retrying} onClick={onRetry}>{retryLabel}</Button>
    : null
  if (variant === "row") {
    return (
      // 行形同时间线系统行（feedback#errors「更早的内容加载失败 · 重试」）：两侧 --line 细线
      <div role="alert" data-slot="inline-error" data-variant="row"
        className={cn("flex items-center gap-3 py-2 text-[12.5px] text-ink-3 before:h-px before:flex-1 before:bg-line after:h-px after:flex-1 after:bg-line", className)} {...rest}>
        <span>
          {title}
          {onRetry && retryLabel && <>
            {" · "}
            <button type="button" title={retryTitle} className="hx-focus cursor-pointer rounded-[var(--r-xs)] text-accent-ink live:hover:underline disabled:cursor-default"
              disabled={retrying} aria-busy={retrying || undefined} onClick={onRetry}>{retryLabel}</button>
          </>}
        </span>
      </div>
    )
  }
  // 有说明时补救按钮另起一行（feedback#errors 块形）；只有标题时按钮跟在标题行尾。
  const footer = Boolean(desc)
  return (
    <div role="alert" data-slot="inline-error" data-variant="block" className={cn("rounded-[var(--r-card)] bg-red-tint px-3 py-2.5", className)} {...rest}>
      <div className="flex items-center gap-2">
        <Icon name="circle-x" size={16} className="shrink-0 text-red" />
        <b className="min-w-0 flex-1 text-[13px] font-medium text-ink">
          {title}{code && <span className="font-normal">（{code}）</span>}
        </b>
        {meta && <span className="font-mono text-[12px] text-ink-3">{meta}</span>}
        {!footer && actions}
        {!footer && retry}
      </div>
      {desc && <p className="mt-1 text-[12.5px] text-ink-2">{desc}</p>}
      {footer && (actions || retry) && (
        <div className="mt-2.5 flex items-center gap-1.5">
          {actions}
          {retry && <><span className="flex-1" />{retry}</>}
        </div>
      )}
    </div>
  )
}

export { InlineError }
