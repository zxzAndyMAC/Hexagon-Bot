// 用量入口 UsageButton 与触顶横幅 CapBanner（components B4，usage#quick；interaction §10）。
// 入口在侧栏底部设置旁：ok 只图标 · warn ≥80% 橙底百分比 · cap 红底「上限」；圆环只留给上下文仪表。
// 点击：规范是弹出用量卡（D20，票 12）。票 02 过渡：直接打开现有的「用量详情」页签（主区），对账表票 02 记录。
// 横幅：输入条正上方，触顶时出现——会阻断新的模型请求，所以不能只靠角落图标。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { fmtYuan, MC_PER_CENT } from '../../usage'
import { usageTier } from '../../usageEntry'
import { Icon } from '../Icon'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

function openUsageDetail(title: string) {
  useUiStore.getState().openTab({ id: 'usage', kind: 'usage', title })
}

export function UsageButton() {
  const { t } = useTranslation()
  const total = useUiStore((s) => s.usageTotal)
  const { tier, pct } = usageTier(total)
  const limit = total?.limit_cents != null ? fmtYuan(total.limit_cents * MC_PER_CENT) : null
  const label = tier === 'cap' ? t('usageEntry.capLabel')
    : pct != null ? t('usageEntry.label', { pct })
    : t('usageEntry.labelPlain')
  const title = !total ? t('usageEntry.labelPlain')
    : limit ? t('usageEntry.title', { spent: fmtYuan(total.spent_mc), limit })
    : t('usageEntry.titleNoLimit', { spent: fmtYuan(total.spent_mc) })
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          data-usage-tier={tier}
          aria-label={label}
          onClick={() => openUsageDetail(t('usage.detail'))}
          className={cn(
            'hx-press hx-focus inline-flex h-[26px] shrink-0 cursor-pointer items-center gap-1 rounded-[8px] px-1.5 font-mono text-[11.5px] leading-none font-medium text-ink-2',
            (tier === 'ok' || tier === 'unknown') && 'live:hover:bg-hover live:hover:text-ink',
            tier === 'warn' && 'bg-orange-tint pr-2 text-orange',
            tier === 'cap' && 'bg-red-tint pr-2 font-sans font-semibold text-red',
          )}
        >
          <Icon name="chart-column" size={14} />
          {tier === 'warn' && `${pct}%`}
          {tier === 'cap' && t('usageEntry.cap')}
        </button>
      </TooltipTrigger>
      <TooltipContent>{title}</TooltipContent>
    </Tooltip>
  )
}

export function CapBanner({ onRaise }: { onRaise: () => void }) {
  const { t } = useTranslation()
  const total = useUiStore((s) => s.usageTotal)
  if (usageTier(total).tier !== 'cap' || total?.limit_cents == null) return null
  return (
    <div
      role="alert"
      data-cap-banner
      className="mx-auto mb-2 flex w-full max-w-[720px] items-center gap-2 rounded-[12px] bg-red-tint py-2 pr-2 pl-3 text-[13px] text-red"
    >
      <Icon name="warn" size={16} />
      <span className="min-w-0 flex-1">{t('usage.capBanner', { limit: fmtYuan(total.limit_cents * MC_PER_CENT) })}</span>
      <Button size="xs" onClick={() => openUsageDetail(t('usage.detail'))}>{t('usageEntry.view')}</Button>
      <Button size="xs" onClick={onRaise}>{t('usageEntry.raise')}</Button>
    </div>
  )
}
