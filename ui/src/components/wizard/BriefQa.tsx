// 答问卡 qcard 与折叠行 answered（components E10，entry#wz-brief）。
// 2026-10-11 负责人选定「卡内逐题」：答问在向导卡片内原地进行，取代右侧并列弹层——
// 答问期间草稿框不能编辑，并排只会挤（reconciliation §2 #5）。头尾固定、中间滚动、最高 300。
// 数字键 1–9 选择、↵ 继续是卡内局部键（沿用 Approval Card 的数字选择），不进全局 keymap。
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { BriefQuestion } from '../../api'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { Input } from '@/components/ui/input'
import { Kbd } from '@/components/ui/kbd'
import { Icon, type IconName } from '../Icon'

/// 折叠行：inset 底、圆角 10，左图标 + 一行省略的摘要 + 右侧动作。
export function AnsweredRow({ icon, tone, text, children }: {
  icon: IconName
  tone?: 'ok'
  text: ReactNode
  children?: ReactNode
}) {
  return (
    <div className="flex min-h-9 items-center gap-2 rounded-[10px] bg-inset px-2.5 py-1.5 text-[12.5px] text-ink-2 shadow-[inset_0_0_0_1px_var(--line-soft)]">
      <Icon name={icon} size={12} className={tone === 'ok' ? 'text-green' : undefined} />
      <span className="min-w-0 flex-1 truncate">{text}</span>
      {children}
    </div>
  )
}

export function BriefQaCard({ questions, index, answers, onIndex, onAnswer, onContinue, onSkip }: {
  questions: BriefQuestion[]
  index: number
  answers: Record<number, string>
  onIndex: (i: number) => void
  onAnswer: (i: number, v: string) => void
  onContinue: () => void
  onSkip: () => void
}) {
  const { t } = useTranslation()
  const q = questions[index]
  const answer = answers[index] ?? ''
  const custom = !!answer && !q.options.includes(answer)
  const [other, setOther] = useState(false)
  const showOther = other || custom
  const card = useRef<HTMLDivElement>(null)
  const can = !!answer.trim()

  // 换题即把焦点放回卡片，数字键与 ↵ 直接可用（不抢输入框里的焦点）
  useEffect(() => {
    const active = document.activeElement
    if (active instanceof HTMLInputElement && card.current?.contains(active)) return
    card.current?.focus({ preventScroll: true })
  }, [index])

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.metaKey || e.ctrlKey || e.altKey || e.defaultPrevented) return
    const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      if (can) onContinue()
      return
    }
    // 数字键在「其他」输入框里是正文，不拦截
    if (typing || e.shiftKey) return
    const n = Number(e.key)
    if (Number.isInteger(n) && n >= 1 && n <= Math.min(9, q.options.length)) {
      e.preventDefault()
      setOther(false)
      onAnswer(index, q.options[n - 1])
    }
  }

  return (
    <div
      ref={card}
      tabIndex={-1}
      data-qa-cards
      aria-label={t('wizard.qaTitle')}
      onKeyDown={onKeyDown}
      className="flex max-h-[300px] flex-col rounded-[12px] bg-surface shadow-[var(--shadow-card)] outline-none"
    >
      <div className="flex flex-none items-center gap-2.5 border-b border-line-soft px-3.5 py-3">
        <b id={`qa-q-${index}`} className="min-w-0 flex-1 text-[14px] font-semibold">{q.question}</b>
        <span data-qa-progress className="font-mono text-[12px] text-ink-3">{index + 1} / {questions.length}</span>
      </div>
      <div role="radiogroup" aria-labelledby={`qa-q-${index}`} className="flex min-h-0 flex-col gap-0.5 overflow-auto p-2">
        {q.options.map((opt, i) => {
          const on = answer === opt
          return (
            <button
              key={opt}
              type="button"
              role="radio"
              aria-checked={on}
              data-qa-option
              onClick={() => { setOther(false); onAnswer(index, opt) }}
              className={cn(
                'hx-press hx-focus flex min-h-9 cursor-pointer items-center gap-2.5 rounded-[8px] px-2 text-left text-[13px] live:hover:bg-hover',
                on && 'bg-hover',
              )}
            >
              <span
                aria-hidden
                className={cn(
                  'grid size-[18px] flex-none place-items-center rounded-[5px] font-mono text-[10.5px] font-medium',
                  on ? 'bg-primary text-white' : 'text-ink-3 shadow-[inset_0_0_0_1px_var(--line)]',
                )}
              >
                {i + 1}
              </span>
              {opt}
            </button>
          )
        })}
        {showOther ? (
          <div className="flex min-h-9 items-center gap-2.5 px-2">
            <span aria-hidden className="grid size-[18px] flex-none place-items-center text-ink-3"><Icon name="edit" size={12} /></span>
            <Input
              autoFocus={other}
              aria-label={t('wizard.qaOther')}
              placeholder={t('wizard.qaOther')}
              value={custom ? answer : ''}
              onChange={(e) => onAnswer(index, e.target.value)}
            />
          </div>
        ) : (
          <button
            type="button"
            data-qa-other
            onClick={() => setOther(true)}
            className="hx-press hx-focus flex min-h-9 cursor-pointer items-center gap-2.5 rounded-[8px] px-2 text-left text-[13px] text-ink-3 live:hover:bg-hover"
          >
            <span aria-hidden className="grid size-[18px] flex-none place-items-center"><Icon name="edit" size={12} /></span>
            {t('wizard.qa.other')}
          </button>
        )}
      </div>
      <div className="flex flex-none items-center gap-2 border-t border-line-soft py-2.5 pr-3 pl-3.5">
        <span className="inline-flex items-center gap-0.5">
          <IconButton size="sm" label={t('wizard.qa.prev')} disabled={index === 0} onClick={() => onIndex(index - 1)}>
            <Icon name="chevron-left" />
          </IconButton>
          <IconButton size="sm" label={t('wizard.qa.next')} disabled={index >= questions.length - 1} onClick={() => onIndex(index + 1)}>
            <Icon name="chevron-right" />
          </IconButton>
        </span>
        <span className="min-w-0 flex-1 truncate text-[12px] text-ink-3">
          {t('wizard.qa.keysHint', { n: Math.min(9, q.options.length) })}
        </span>
        <Button size="sm" variant="quiet" onClick={onSkip}>{t('wizard.qaSkip')}</Button>
        <Button size="sm" variant="primary" disabled={!can} onClick={onContinue} data-qa-continue>
          {t('wizard.qa.continue')}<Kbd>↵</Kbd>
        </Button>
      </div>
    </div>
  )
}
