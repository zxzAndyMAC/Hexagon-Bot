import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api } from '../api'

export function TopBar({ onSettings }: { onSettings: () => void }) {
  const { t } = useTranslation()
  const { projectName, autonomy, usageTotal, pending, refresh } = useUiStore()
  const fmt = (mc: number) => `¥${(mc / 100000).toFixed(1)}`

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
      <span className="dim mono" style={{ fontSize: 12 }}>
        {t('topbar.usage')} {usageTotal ? `${fmt(usageTotal.spent_mc)}${usageTotal.limit_cents ? ` / ¥${usageTotal.limit_cents / 100}` : ''}` : '—'}
      </span>
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
        onClick={async () => { await api.sleepAll(); await refresh() }}
      >
        {t('topbar.sleepAll')}
      </button>
      <button className="btn" onClick={onSettings}>{t('topbar.settings')}</button>
    </header>
  )
}
