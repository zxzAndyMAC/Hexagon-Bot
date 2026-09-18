import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api } from '../api'
import { capReached, centsToMc } from '../usage'

export function TopBar({ onSettings }: { onSettings: () => void }) {
  const { t } = useTranslation()
  const { projectName, autonomy, usageTotal, pending, refresh, setRailOpen, setSideTab, railOpen } = useUiStore()
  const fmt = (mc: number) => `¥${(mc / 100000).toFixed(1)}`
  const limitMc = usageTotal?.limit_cents != null ? centsToMc(usageTotal.limit_cents) : null
  const capped = capReached(usageTotal?.spent_mc ?? 0, usageTotal?.limit_cents)

  const locatePending = () => {
    const zone = document.getElementById('pending-zone')
    zone?.scrollTo({ top: 0, behavior: 'smooth' })
    zone?.classList.add('flash-zone')
    setTimeout(() => zone?.classList.remove('flash-zone'), 1400)
  }

  return (
    <header className="row-line" style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 14px', background: 'var(--bg-1)' }}>
      <strong style={{ fontWeight: 510 }}>{projectName}</strong>
      <span className="chip mono">{t('topbar.pack')} v3</span>
      <span className="chip amber">{t(`autonomy.${autonomy}`)}</span>
      <button
        className={`chip ${capped ? 'err' : 'mono'}`}
        style={{ cursor: 'pointer', fontSize: 11, ...(capped ? { animation: 'pulse-amber 1.6s infinite' } : {}) }}
        title={t('usage.openDashboard')}
        onClick={() => {
          setSideTab('usage')
          if (!railOpen) setRailOpen(true)
        }}
      >
        {capped ? `⚠ ${t('usage.capHit')}` : t('topbar.usage')}{' '}
        {usageTotal ? `${fmt(usageTotal.spent_mc)}${limitMc != null ? ` / ¥${usageTotal!.limit_cents! / 100}` : ''}` : '—'}
      </button>
      <div style={{ flex: 1 }} />
      {pending.length > 0 && (
        <button
          className="chip amber"
          style={{ cursor: 'pointer', border: '1px solid var(--accent-border)', animation: 'pulse-amber 1.6s infinite' }}
          onClick={locatePending}
        >
          ⚠ {t('cards.pending', { count: pending.length })}
        </button>
      )}
      <button
        className="btn danger"
        onClick={async () => {
          if (confirm(t('topbar.sleepAllConfirm'))) {
            await api.sleepAll()
            await refresh()
          }
        }}
      >
        {t('topbar.sleepAll')}
      </button>
      <button className="btn" onClick={onSettings}>{t('topbar.settings')}</button>
    </header>
  )
}
