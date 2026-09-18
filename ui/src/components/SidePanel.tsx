import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'

export function SidePanel() {
  const { t } = useTranslation()
  const { artifacts, team } = useUiStore()
  return (
    <aside className="panel" style={{ width: 260, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div className="row-line" style={{ padding: '8px 12px', fontWeight: 510 }}>{t('side.artifacts')}</div>
      <div style={{ flex: 1, overflowY: 'auto' }}>
        {artifacts.map((a) => (
          <div key={a.id} style={{ padding: '5px 12px', display: 'flex', gap: 8, alignItems: 'baseline' }}>
            <span className="mono" style={{ fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{a.path}</span>
            <span className={`chip ${a.status === 'stamped' ? 'amber' : a.status === 'superseded' ? '' : 'ok'}`} style={{ fontSize: 10, marginLeft: 'auto' }}>
              {t(`side.${a.status}`, a.status)}
            </span>
          </div>
        ))}
      </div>
      <div className="row-line" style={{ padding: '8px 12px', fontWeight: 510, borderTop: '1px solid var(--border)' }}>{t('side.team')}</div>
      <div style={{ overflowY: 'auto', maxHeight: '40%' }}>
        {team.map((m) => (
          <div key={m.id} style={{ padding: '5px 12px', display: 'flex', gap: 8, alignItems: 'center', opacity: m.status === 'sleeping' ? 0.5 : 1 }}>
            <span className={`dot ${m.status === 'active' ? 'on' : 'off'}`} />
            <span>{m.role}</span>
            <span className="dim3 mono" style={{ fontSize: 10, marginLeft: 'auto' }}>{m.model_slot || '—'}</span>
          </div>
        ))}
      </div>
    </aside>
  )
}
