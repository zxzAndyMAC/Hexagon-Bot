// 右栏「阶段」页（ADR 0069）。只读。人不再从这里拨指针。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { Icon } from './Icon'

export function StageRail() {
  const { t } = useTranslation()
  const stages = useUiStore((s) => s.stages)

  return (
    <div data-stage-rail style={{ padding: '8px 12px' }}>
      {stages.length === 0 ? (
        <div className="dim3" style={{ fontSize: 11, lineHeight: 1.5 }}>{t('side.noStages')}</div>
      ) : (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4 }}>
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
        </div>
      )}
    </div>
  )
}
