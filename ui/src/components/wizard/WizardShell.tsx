// 向导外壳 WizardShell（components E9，entry#wz-provider/wz-brief/wz-roles/wz-flow）：
// 880 宽卡片 + 210 宽竖向步骤轨 + 内容区 + 底栏。卡片固定宽、最小 560 高，切步不跳动。
// 已完成步可点回跳（未来步不可点——跳步会绕过放行条件）；AI 在途（locked）时回跳与
// 上一步 / 下一步一起锁（owner 2026-09-25，AI 在途锁跳步）。
import { useEffect, useRef, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { Icon } from '../Icon'
import type { Step } from './draft'

export function WizardShell({ steps, step, locked, onJump, children, foot }: {
  steps: readonly Step[]
  step: Step
  locked: boolean
  onJump: (s: Step) => void
  children: ReactNode
  foot: ReactNode
}) {
  const { t } = useTranslation()
  const idx = steps.indexOf(step)
  return (
    <div
      data-wizard
      data-tauri-drag-region
      className="fixed inset-0 z-[60] flex justify-center overflow-auto bg-[var(--app-main)] px-4 pt-14 pb-6 text-ink dark:bg-page"
    >
      <div className="flex h-fit max-h-[calc(100vh-80px)] min-h-[560px] w-[880px] max-w-full overflow-hidden rounded-[16px] bg-surface shadow-[var(--shadow-overlay)]">
        <nav aria-label={t('wizard.shell.title')} className="w-[210px] flex-none border-r border-line-soft bg-inset px-4 py-[22px]">
          <div className="mb-3 text-[13px] font-semibold">{t('wizard.shell.title')}</div>
          <ol className="m-0 list-none p-0">
            {steps.map((s, i) => {
              const done = i < idx
              const current = i === idx
              const jumpable = done && !locked
              const body = (
                <>
                  <span
                    className={cn(
                      'grid size-5 flex-none place-items-center rounded-full text-[11px] font-semibold',
                      done && 'bg-ink text-page',
                      current && 'text-accent shadow-[inset_0_0_0_1.5px_var(--accent)]',
                      !done && !current && 'shadow-[inset_0_0_0_1.5px_var(--line-strong)]',
                    )}
                  >
                    {done ? <Icon name="check" size={12} /> : i + 1}
                  </span>
                  {t(`wizard.rail.${s}`)}
                </>
              )
              const row = cn(
                'flex h-[34px] w-full items-center gap-2.5 rounded-[var(--r-sm)] px-1.5 text-left text-[13px]',
                done ? 'text-ink-2' : current ? 'font-medium text-ink' : 'text-ink-3',
              )
              return (
                <li key={s} data-step={s} aria-current={current ? 'step' : undefined}>
                  {jumpable ? (
                    <button type="button" className={cn(row, 'hx-press hx-focus cursor-pointer live:hover:bg-hover')} onClick={() => onJump(s)}>
                      {body}
                    </button>
                  ) : (
                    <div className={row}>{body}</div>
                  )}
                </li>
              )
            })}
          </ol>
        </nav>
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex min-h-0 flex-1 flex-col overflow-auto">{children}</div>
          <div className="flex flex-none items-center gap-2 border-t border-line-soft px-5 py-3.5">{foot}</div>
        </div>
      </div>
    </div>
  )
}

/// 内容区标题（18/600）+ 说明（13 ink-2）。切步后焦点到标题（E9 聚焦），读屏先读到新步骤名；
/// 打开向导时不抢焦点（focus=false）。
export function StepHeading({ title, lead, focus }: { title: string; lead?: ReactNode; focus: boolean }) {
  const ref = useRef<HTMLHeadingElement>(null)
  useEffect(() => {
    if (focus) ref.current?.focus({ preventScroll: true })
  }, [focus])
  return (
    <>
      <h3 ref={ref} tabIndex={-1} className="mb-1 text-[18px] font-semibold tracking-[-.01em] outline-none">{title}</h3>
      {lead && <p className="mb-5 text-[13px] text-ink-2">{lead}</p>}
    </>
  )
}
