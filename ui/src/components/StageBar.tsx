import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api } from '../api'
import { Icon } from './Icon'

export function StageBar() {
  const { t } = useTranslation()
  const { stages, refresh } = useUiStore()
  const active = stages.find((s) => s.state === 'active' || s.state === 'waiting_stamp')
  const nextPending = stages.find((s) => s.state === 'pending')

  const act = async (f: () => Promise<unknown>) => { await f(); await refresh() }

  return (
    <div className="row-line" style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '8px 14px', overflowX: 'auto' }}>
      {stages.map((s) => (
        <span
          key={s.run_id}
          className="chip"
          style={
            s.state === 'active' || s.state === 'waiting_stamp'
              ? { color: 'var(--accent)', borderColor: 'var(--accent-border)', background: 'var(--accent-soft)' }
              : s.state === 'done'
                ? { color: 'var(--ok)' }
                : s.state === 'skipped'
                  ? { opacity: 0.45 }
                  : {}
          }
        >
          {s.state === 'done' && <Icon name="check" size={9} />}
          {s.state === 'waiting_stamp' && <><Icon name="stamp" size={9} /> {t('stage.stampPoint')} </>}
          {s.stage}
        </span>
      ))}
      <div style={{ flex: 1 }} />
      {active && (
        <>
          <button className="btn" onClick={() => act(() => api.rewind(active.seq - 1))}>{t('stage.rewind')}</button>
          <button className="btn" onClick={() => act(api.skip)}>{t('stage.skip')}</button>
          <button className="btn" onClick={() => act(api.pause)}>{t('stage.pause')}</button>
          {active.state === 'waiting_stamp' && (
            <button className="btn primary" onClick={() => act(api.stamp)}>{t('cards.stamp')}</button>
          )}
        </>
      )}
      {!active && nextPending && (
        <button className="btn primary" style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }} onClick={() => act(() => api.openStage(nextPending.seq))}>
          <Icon name="chevron-right" size={11} /> {nextPending.stage}
        </button>
      )}
      {!active && !nextPending && stages.length > 0 && (
        <button className="btn" onClick={() => act(api.resume)}>{t('stage.resume')}</button>
      )}
    </div>
  )
}
