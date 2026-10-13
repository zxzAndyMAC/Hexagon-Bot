import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { bindingFor, formatBinding, matches, normalizeEvent } from '../keymap'
import { Icon } from './Icon'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ShortcutKbd } from '@/components/ui/kbd'

// 票 20（方向卡 5）：快捷键训练场——mock 待决卡 + 真绑定练习。
// 隔离双保险：①组件内 keydown 命中即 stopPropagation（不冒泡到 window
// 级分发）；②modalScope=settings 时全局裁决分发本就不生效（票 02）。
// 绝不触真 api.answerPermission——命中只记数+✓。
// ui-redesign 票 11（components E7，settings#keys）：换成 D8 独立形模拟卡（shield 橙）+ 副行
// 「不会触发真实裁决 · 已正确 n / m」+ 命中 ok chip「⌘↵ 命中」。m = 卡上按下的带修饰键组合次数。
// 卡脚按钮只是键位示意：点它把焦点交回卡片，不计数（练的是键盘）。
export function PracticeGround() {
  const { t } = useTranslation()
  const [hits, setHits] = useState(0)
  const [tries, setTries] = useState(0)
  const [hit, setHit] = useState<string | null>(null)
  const card = useRef<HTMLDivElement>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  // 每次渲染重读绑定：重映射后提示与判定跟随新键位。
  const approveB = bindingFor('approve')
  const rejectB = bindingFor('reject')
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const combo = normalizeEvent(e)
    if (!combo || !/(^|\+)(mod|alt)\+/.test(combo)) return
    const which = matches(e, approveB) ? approveB : matches(e, rejectB) ? rejectB : null
    e.preventDefault()
    e.stopPropagation()
    setTries((n) => n + 1)
    if (!which) return
    setHits((n) => n + 1)
    setHit(which)
    clearTimeout(timer.current)
    timer.current = setTimeout(() => setHit(null), 1200)
  }
  const refocus = () => card.current?.focus()
  return (
    <div
      ref={card}
      data-practice
      tabIndex={0}
      role="group"
      aria-label={t('settings.practiceTitle')}
      onKeyDown={onKeyDown}
      className="hx-focus overflow-hidden rounded-[14px] bg-surface shadow-[var(--shadow-card)] outline-none"
    >
      <div className="flex items-center gap-2.5 px-4 pt-3.5 pb-1.5">
        <span className="grid size-7 flex-none place-items-center rounded-[8px] bg-orange-tint text-orange">
          <Icon name="shield" size={14} />
        </span>
        <div className="min-w-0 flex-1">
          <h4 className="text-[14px] font-semibold text-ink">{t('settings.practiceCard')}</h4>
          <div className="text-[12px] text-ink-3">
            {t('settingsUi.practiceSub', { hits, tries })}
          </div>
        </div>
        {hit && (
          <Badge variant="ok" role="status">
            <Icon name="check" size={12} />
            {t('settingsUi.practiceHit', { key: formatBinding(hit) })}
          </Badge>
        )}
      </div>
      <div className="px-4 pb-3 text-[13px] text-ink-2">
        {t('settings.practiceHint', { approve: formatBinding(approveB), reject: formatBinding(rejectB) })}
        <span className="sr-only">{t('settings.practiceCount', { count: hits })}</span>
      </div>
      <div className="flex items-center gap-2 border-t border-line-soft py-2.5 pr-3 pl-4">
        <span className="flex-1" />
        <Button size="sm" variant="quiet" tabIndex={-1} onClick={refocus}>
          {t('settingsUi.practiceReject')}<ShortcutKbd binding={rejectB} />
        </Button>
        <Button size="sm" variant="primary" tabIndex={-1} onClick={refocus}>
          {t('settingsUi.practiceApprove')}<ShortcutKbd binding={approveB} />
        </Button>
      </div>
    </div>
  )
}
