import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { Icon } from './Icon'

/** components D11「向导与设置内」：模型请求失败在发起它的位置原地显示，保留输入与缓存。
 * ui-redesign 票 08：向导生成失败（inset 块）与设置参考译文失败（页签内红字）共用这一个组件，
 * 只差 `form`：`inset` = inset 块 + 橙色 warn 头；`text` = 红字行（C5 形）。动作由调用方给。 */
export function ModelRecoveryInline({ form, text, actions, className, ...rest }: {
  form: 'inset' | 'text'
  text: ReactNode
  actions?: ReactNode
  className?: string
} & Record<`data-${string}`, string | undefined>) {
  return <div role={form === 'text' ? 'alert' : undefined} data-recovery-inline={form} {...rest}
    className={cn('flex flex-wrap items-center gap-2 text-[12.5px]',
      form === 'inset' ? 'mt-3 rounded-[var(--r-card)] bg-inset px-3 py-2.5 shadow-[inset_0_0_0_1px_var(--line-soft)]' : 'text-red',
      className)}>
    <Icon name="warn" size={14} className={cn('flex-none', form === 'inset' && 'text-orange')} />
    <span className={cn('min-w-0 flex-1', form === 'inset' && 'text-ink-2')}>{text}</span>
    {actions}
  </div>
}
