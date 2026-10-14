// 电脑操控面板的共用小件（components D19，control#panels；票 ui-redesign 14）。
// ActionButton：A1 按钮 + A15 tooltip「名称 · 当前绑定」（ADR 0051）；禁用时 tooltip 写原因（components §0）。
// CheckRow：Task Rows 勾选行——circleCheck 绿 = 已授权 / alert 橙 = 待处理 + 右侧动作。
import type { ReactNode } from 'react'
import { bindingFor, type ActionId } from '../keymap'
import { Button, type ButtonProps } from '@/components/ui/button'
import { ShortcutKbd } from '@/components/ui/kbd'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { Icon } from './Icon'

export function ActionButton({ label, action, disabledReason, children, ...props }: ButtonProps & {
  label: string
  action?: ActionId
  disabledReason?: string
}) {
  const binding = action ? bindingFor(action) : ''
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button {...props}>{children ?? label}</Button>
      </TooltipTrigger>
      <TooltipContent>
        <span>{props.disabled && disabledReason ? disabledReason : label}</span>
        {binding && !(props.disabled && disabledReason) && <ShortcutKbd binding={binding} />}
      </TooltipContent>
    </Tooltip>
  )
}

export function CheckRow({ ok, label, children, ...props }: { ok: boolean; label: string; children?: ReactNode } & Record<`data-${string}`, string | boolean | undefined>) {
  return (
    <div className="flex h-8 items-center gap-2 px-2.5 text-[12.5px]" data-ok={ok || undefined} {...props}>
      <span className={ok ? 'text-green' : 'text-orange'}><Icon name={ok ? 'circle-check' : 'warn'} size={14} /></span>
      <span className="min-w-0 flex-1 truncate text-ink" title={label}>{label}</span>
      {children}
    </div>
  )
}

/** inset 块（proto .inset）：--inset 底、--r-card、--line-soft 内环。 */
export const insetBlock = 'rounded-[var(--r-card)] bg-inset shadow-[inset_0_0_0_1px_var(--line-soft)]'
