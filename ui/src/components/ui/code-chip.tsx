// 代码 chip CodeChip（components A5）：22 高、padding 0 7、圆角 6、11.5px 等宽、--ink-2 字、--inset 底、--line 内环，
// 超长省略（title 给全文）。文件 chip 变体：文字 --ink，后接 +N（--green）/ −N（--red）。
// 可点击时（传 onClick）渲染成按钮，带悬停/按下/焦点；missing = 路径已不存在（删除线 + --ink-3，补设计）。
import { cn } from "@/lib/utils"

type CodeChipProps = {
  children: string
  file?: boolean
  added?: number
  removed?: number
  missing?: boolean
  missingLabel?: string
  onClick?: () => void
  className?: string
}

function CodeChip({ children, file, added, removed, missing, missingLabel, onClick, className }: CodeChipProps) {
  const cls = cn(
    "inline-flex h-[22px] max-w-full items-center gap-1.5 overflow-hidden rounded-[6px] bg-inset px-[7px] font-mono text-[11.5px] whitespace-nowrap text-ink-2 [--hx-shadow:inset_0_0_0_1px_var(--line)] shadow-[var(--hx-shadow)]",
    file && "text-ink",
    missing && "text-ink-3 line-through",
    onClick && "hx-press hx-focus cursor-pointer live:hover:bg-hover",
    className
  )
  const body = (
    <>
      <span className="truncate">{children}</span>
      {added !== undefined && <span className="text-green">+{added}</span>}
      {removed !== undefined && <span className="text-red">−{removed}</span>}
    </>
  )
  const title = missing && missingLabel ? missingLabel : children
  return onClick ? (
    <button type="button" data-slot="code-chip" className={cls} title={title} onClick={onClick}>
      {body}
    </button>
  ) : (
    <span data-slot="code-chip" className={cls} title={title}>
      {body}
    </span>
  )
}

export { CodeChip }
