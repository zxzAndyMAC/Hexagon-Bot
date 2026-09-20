import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { useUiStore } from '../store'
import { PackEditor } from './PackEditor'
import { api } from '../api'
import { Icon } from './Icon'

export function StageBar() {
  const { t } = useTranslation()
  const { stages, invalidate } = useUiStore()
  const [editingPack, setEditingPack] = useState(false)
  const active = stages.find((s) => s.state === 'active' || s.state === 'waiting_stamp')
  const interrupted = stages.some((s) => s.state === 'interrupted')
  const nextPending = stages.find((s) => s.state === 'pending')

  const act = async (f: () => Promise<unknown>) => { await f(); await invalidate() }

  return (
    <div className="row-line" style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '8px 14px', overflowX: 'auto' }}>
      {stages.map((s) => (
        <span
          key={s.run_id}
          className="chip"
          style={
            s.state === 'active' || s.state === 'waiting_stamp' || s.state === 'interrupted'
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
          {s.state === 'interrupted' && <><Icon name="warn" size={9} /> {t('stage.interrupted')} </>}
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
      {!active && nextPending && !interrupted && (
        <button className="btn primary" style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }} onClick={() => act(() => api.openStage(nextPending.seq))}>
          <Icon name="chevron-right" size={11} /> {nextPending.stage}
        </button>
      )}
      {!active && !nextPending && stages.length > 0 && !interrupted && (
        <button className="btn" onClick={() => act(api.resume)}>{t('stage.resume')}</button>
      )}
      {stages.length > 0 && (
        <button
          className="icon-btn"
          title={t('pack.editor')}
          onClick={() => setEditingPack(true)}
        >
          <Icon name="edit" size={12} />
        </button>
      )}
      {editingPack && <PackEditor onClose={() => setEditingPack(false)} />}
    </div>
  )
}
