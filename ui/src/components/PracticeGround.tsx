import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { bindingFor, formatBinding, matches } from '../keymap'
import { Icon } from './Icon'

// 票 20（方向卡 5）：快捷键训练场——mock 待决卡 + 真绑定练习。
// 隔离双保险：①组件内 keydown 命中即 stopPropagation（不冒泡到 window
// 级分发）；②modalScope=settings 时全局裁决分发本就不生效（票 02）。
// 绝不触真 api.answerPermission——命中只记数+✓。
export function PracticeGround() {
  const { t } = useTranslation()
  const [hits, setHits] = useState(0)
  const [ok, setOk] = useState(false)
  // 每次渲染重读绑定：重映射后提示与判定跟随新键位。
  const approveB = bindingFor('approve')
  const rejectB = bindingFor('reject')
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (!matches(e, approveB) && !matches(e, rejectB)) return
    e.preventDefault()
    e.stopPropagation()
    setHits((n) => n + 1)
    setOk(true)
    setTimeout(() => setOk(false), 600)
  }
  return (
    <div
      className="card-ask"
      tabIndex={0}
      role="group"
      aria-label={t('settings.practiceTitle')}
      onKeyDown={onKeyDown}
      style={{ marginTop: 14, padding: '10px 14px', outline: 'none' }}
    >
      <div style={{ fontWeight: 510, color: 'var(--accent)', fontSize: 12, display: 'flex', alignItems: 'center', gap: 6 }}>
        <Icon name="warn" size={12} /> {t('settings.practiceCard')}
        {ok && <span className="chip ok" style={{ fontSize: 10 }}>✓</span>}
      </div>
      <div className="dim3" style={{ fontSize: 11, margin: '6px 0' }}>
        {t('settings.practiceHint', { approve: formatBinding(approveB), reject: formatBinding(rejectB) })}
      </div>
      <div className="dim3" style={{ fontSize: 11 }}>{t('settings.practiceCount', { count: hits })}</div>
    </div>
  )
}

