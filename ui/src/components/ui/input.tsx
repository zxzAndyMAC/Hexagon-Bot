// 输入框 Field（components A9）：34 高、padding 0 11、圆角 --r-sm、--field 底、
// 阴影 --shadow-inset-field + --line 内环、13px；可内嵌前置图标、后置图标按钮、后置状态。
// 悬停内环转 --line-strong；聚焦底转 --surface、阴影换焦点环；禁用文字 --ink-3；
// 加载后置位换 ring + 进行时文案；错误内环 --red + aria-invalid（Help 改红字由 Field 给）。
import * as React from "react"
import { cn } from "@/lib/utils"
import { Spinner } from "@/components/ui/spinner"

const fieldShell = cn(
  "flex w-full max-w-full items-center gap-2 rounded-[var(--r-sm)] bg-field text-[13px] text-ink",
  "shadow-[var(--shadow-inset-field),inset_0_0_0_1px_var(--line)] transition-[background-color,box-shadow] duration-150 ease-hx",
  "hover:shadow-[var(--shadow-inset-field),inset_0_0_0_1px_var(--line-strong)]",
  "focus-within:bg-surface focus-within:shadow-[var(--focus-ring)] hover:focus-within:shadow-[var(--focus-ring)]",
  "has-[[aria-invalid=true]]:not-focus-within:shadow-[var(--shadow-inset-field),inset_0_0_0_1px_var(--red)]!",
  "has-[:disabled]:text-ink-3 has-[:disabled]:opacity-45 has-[:disabled]:hover:shadow-[var(--shadow-inset-field),inset_0_0_0_1px_var(--line)]"
)
const control = "min-w-0 flex-1 bg-transparent outline-none placeholder:text-ink-3 disabled:cursor-default"

type InputProps = React.ComponentProps<"input"> & {
  leading?: React.ReactNode
  trailing?: React.ReactNode
  /** 后置位换 ring + 进行时文案（如「检测中…」）。 */
  loadingLabel?: string
}

function Input({ className, leading, trailing, loadingLabel, ...props }: InputProps) {
  return (
    <div data-slot="input" className={cn(fieldShell, "h-[34px] px-[11px] [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-ink-3", className)}>
      {leading}
      <input className={cn(control, "h-full")} aria-busy={loadingLabel ? true : undefined} {...props} />
      {loadingLabel ? (
        <span className="inline-flex items-center gap-1.5 text-[12px] whitespace-nowrap text-ink-3">
          <Spinner />
          {loadingLabel}
        </span>
      ) : trailing}
    </div>
  )
}

/** textarea：自适应高、padding 9 11、行高 1.55。 */
function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <div data-slot="textarea" className={cn(fieldShell, "items-stretch px-[11px] py-[9px]", className)}>
      <textarea className={cn(control, "field-sizing-content min-h-[3lh] resize-none leading-[1.55]")} {...props} />
    </div>
  )
}

/** Label 12.5/500 下距 6；Help 12 --ink-3 上距 5；error 时 Help 改 --red 写原因与下一步。 */
function Field({
  label,
  help,
  error,
  htmlFor,
  className,
  children,
}: {
  label?: React.ReactNode
  help?: React.ReactNode
  error?: React.ReactNode
  htmlFor?: string
  className?: string
  children: React.ReactNode
}) {
  const noteId = React.useId()
  const note = error ?? help
  // 说明 / 错误挂到控件的 aria-describedby，读屏聚焦时一并读出原因
  const control = note && React.isValidElement<{ "aria-describedby"?: string }>(children)
    ? React.cloneElement(children, { "aria-describedby": noteId })
    : children
  return (
    <div data-slot="field" data-invalid={error ? true : undefined} className={cn("flex flex-col", className)}>
      {label && <label htmlFor={htmlFor} className="mb-1.5 block text-[12.5px] font-medium text-ink">{label}</label>}
      {control}
      {error ? (
        <div id={noteId} role="alert" className="mt-[5px] text-[12px] text-red">{error}</div>
      ) : help ? (
        <div id={noteId} className="mt-[5px] text-[12px] text-ink-3">{help}</div>
      ) : null}
    </div>
  )
}

export { Field, Input, Textarea, fieldShell }
