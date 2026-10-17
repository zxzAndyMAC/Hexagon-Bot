import { useTranslation } from 'react-i18next'
import { useDelegation } from '../useDelegation'
import { ManagedReport } from './ManagedReport'
import { Badge } from '@/components/ui/badge'
import { Icon } from './Icon'
import './ProjectDelegation.css'

/** Settings → Permissions: delegation status and report only. The switch
 * lives in the approval-mode menu (2026-10-11 UI alignment: one place to set
 * how much is handed over).
 * ui-redesign 票 11（components E5「托管状态块」补设计）：E1 行「项目托管」+ 状态 chip
 * （托管中 info / 已暂停 neutral / 未开启 neutral）+ 说明 + D9 报告折叠。报告本体（ManagedReport）归票 08 换皮。 */
export function ProjectDelegation() {
  const { t } = useTranslation()
  const { status, processing } = useDelegation()
  const on = status?.enabled ?? false
  const paused = on && !!status?.paused
  const state = !on ? t('delegation.off') : paused ? t('delegation.pillPaused') : t('delegation.on')
  return <section aria-label={t('delegation.label')} data-managed-block
    className="mb-5 rounded-[12px] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
    <div className="flex min-h-14 items-center gap-3.5 px-4 py-[13px]">
      <Icon name="bot" size={16} className="flex-none text-ink-2" />
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-medium text-ink">{t('delegation.label')}</div>
        <div className="text-[12.5px] text-ink-3">{t('delegation.whereToSwitch')}</div>
        {on && !paused && processing > 0 && <div role="status" className="text-[12.5px] text-ink-3">{t('delegation.processing', { count: processing })}</div>}
      </div>
      <Badge variant={on && !paused ? 'info' : 'neutral'} data-delegation-state={on ? (paused ? 'paused' : 'on') : 'off'}>{state}</Badge>
    </div>
    <details className="group border-t border-line-soft px-4 py-2.5">
      <summary className="hx-focus flex cursor-pointer list-none items-center gap-1.5 rounded-[var(--r-xs)] text-[12.5px] text-ink-2 live:hover:text-ink">
        <Icon name="chevron-right" size={12} className="transition-transform group-open:rotate-90" />
        {t('settingsUi.managedReport')}
      </summary>
      <div className="pt-2.5">
        <ManagedReport />
      </div>
    </details>
  </section>
}
