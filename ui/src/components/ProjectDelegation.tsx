import { useTranslation } from 'react-i18next'
import { useDelegation } from '../useDelegation'
import { ManagedReport } from './ManagedReport'
import './ProjectDelegation.css'

/** Settings → Permissions: delegation status and report only. The switch
 * lives in the approval-mode menu (2026-10-11 UI alignment: one place to set
 * how much is handed over). */
export function ProjectDelegation() {
  const { t } = useTranslation()
  const { status, processing } = useDelegation()
  const on = status?.enabled ?? false
  const state = !on ? t('delegation.off') : status?.paused ? t('delegation.pillPaused') : t('delegation.on')
  return <section className="delegation" aria-label={t('delegation.label')}>
    <div className="delegation-head">
      <strong>{t('delegation.label')}</strong>
      <span className={on ? 'delegation-on' : undefined} data-delegation-state={on ? (status?.paused ? 'paused' : 'on') : 'off'}>{state}</span>
    </div>
    <p className="delegation-note">{t('delegation.whereToSwitch')}</p>
    {on && !status?.paused && processing > 0 && <p className="delegation-note" role="status">{t('delegation.processing', { count: processing })}</p>}
    <ManagedReport />
  </section>
}
