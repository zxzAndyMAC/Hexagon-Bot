// 设置页共享的类名常量（票 11）：从 kit.tsx 拆出，保持组件文件只导出组件（fast refresh）。
import { cn } from '@/lib/utils'

/** A21 表格：卡片内全宽，表头 12/500 ink-3（padding 9 12，底线 --line），行 padding 10 12、行线 --line-soft、末行无线。 */
export const tableCls = {
  table: 'w-full border-collapse text-[13px]',
  th: 'border-b border-line px-3 py-[9px] text-left text-[12px] font-medium text-ink-3',
  td: 'border-b border-line-soft px-3 py-2.5 align-middle [tr:last-child>&]:border-b-0',
  tr: 'live:hover:bg-hover',
} as const

/** 列表行外观（E2 ld-item）：padding 9 10、margin 1 8、圆角 8、13px；选中 --accent-tint + 左竖条。 */
export function ldItemCls(selected: boolean, dim = false) {
  return cn(
    'hx-focus mx-2 my-px flex cursor-pointer items-center gap-2.5 rounded-[var(--r-sm)] px-2.5 py-[9px] text-[13px] text-ink outline-none',
    selected ? 'bg-accent-tint shadow-[inset_2px_0_0_var(--accent)]' : 'live:hover:bg-hover',
    dim && 'opacity-60',
  )
}

