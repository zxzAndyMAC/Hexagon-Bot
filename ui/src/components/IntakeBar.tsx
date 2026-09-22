import { useTranslation } from 'react-i18next'
import { confirmIntakeDraft } from '../intakeDraft'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'

/** 开场分析附的草案。没有草案时不占高度，输入框始终可打字。 */
export function IntakeBar() {
  const { t } = useTranslation()
  const pending = useUiStore((s) => s.intakeDraft)
  if (!pending) return null
  const hint = t('intake.confirmHint')
  return (
    <div
      data-testid="intake-bar"
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '6px 14px 0',
      }}
    >
      <span className="dim3" style={{ fontSize: 12, flex: 1, minWidth: 0 }}>{hint}</span>
      <button
        className="btn primary"
        title={`${hint} · ${formatBinding(bindingFor('confirmIntake'))}`}
        onClick={() => void confirmIntakeDraft()}
      >
        {t('intake.confirm')}
      </button>
    </div>
  )
}
