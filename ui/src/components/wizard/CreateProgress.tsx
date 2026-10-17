// ⑦ 创建进度（Task Rows，components E10，entry#wz-flow 右半）：完成 = 绿色实心勾，进行中 = 环形，
// 未开始 = 空心圈，失败 = circleX 红 + 原因、后续行保持未开始（票 14：失败停在该步，不进入工作台）。
// 耗时按相邻两次进度回报的间隔计（core 不回报耗时，E10 补设计）。
// gap B5（2026-10-15）：「写入角色」行带计数「N / M」（entry#wz-flow「9 / 9」）。M 是本次提交的角色数——
// core 要么把它们全部落库、要么整单失败回滚（setup.rs create_project_reporting），所以进行中写 0 / M，
// 完成写 M / M；票 10 时以「没有数据源」为由不显示。不另开逐角色回报：落库是一次本地写，没有可见的中间态。
import { useTranslation } from 'react-i18next'
import { CREATE_STEPS, type CreateStep } from '../../createProgress'
import { cn } from '@/lib/utils'
import { Spinner } from '@/components/ui/spinner'
import { Icon } from '../Icon'

export function CreateProgress({ name, busy, done, fail, durations, roleCount }: {
  name: string
  roleCount?: number
  busy: boolean
  done: CreateStep[]
  fail: { step: CreateStep; reason: string } | null
  durations: Partial<Record<CreateStep, number>>
}) {
  const { t } = useTranslation()
  const runningStep = busy && !fail ? CREATE_STEPS.find((x) => !done.includes(x)) : undefined
  return (
    <div className="mt-5 rounded-[12px] bg-surface px-[22px] py-5 shadow-[var(--shadow-card)]">
      <h4 className="text-[15px] font-semibold">{t('wizard.create.title', { name })}</h4>
      <ul data-testid="create-progress" className="mt-4 flex list-none flex-col gap-0.5 p-0">
        {CREATE_STEPS.map((s) => {
          const isDone = done.includes(s)
          const failed = fail?.step === s
          const running = runningStep === s
          const state = failed ? 'failed' : isDone ? 'done' : running ? 'running' : 'pending'
          const ms = durations[s]
          return (
            <li key={s} data-create-step={s} data-state={state} className={cn('flex flex-col', state === 'pending' && 'opacity-50')}>
              <div className="flex h-10 items-center gap-2.5">
                {failed ? (
                  <Icon name="circle-x" size={20} className="text-red" />
                ) : isDone ? (
                  <span className="grid size-5 place-items-center rounded-full bg-green text-white"><Icon name="check" size={12} /></span>
                ) : running ? (
                  <Spinner size={20} />
                ) : (
                  <span className="size-5 rounded-full shadow-[inset_0_0_0_1.5px_var(--line-strong)]" />
                )}
                <span className={cn('flex-1 text-[13px]', running && 'font-medium', failed && 'text-red')}>{t(`wizard.create.${s}`)}</span>
                {s === 'persist_roles' && roleCount != null && (running || isDone) && (
                  <span data-role-count="" className="font-mono text-[12px] text-ink-3">{isDone ? roleCount : 0} / {roleCount}</span>
                )}
                {isDone && ms !== undefined && <span className="font-mono text-[12px] text-ink-3">{(ms / 1000).toFixed(1)}s</span>}
              </div>
              {failed && <div className="pb-2 pl-[30px] text-[12.5px] text-red">{fail.reason}</div>}
            </li>
          )
        })}
      </ul>
      <div className="mt-4 flex items-center gap-2 rounded-[var(--r-card)] bg-canvas px-3 py-2.5 text-[12.5px] text-ink-2">
        <Icon name="info" size={14} />{t('wizard.create.note')}
      </div>
    </div>
  )
}
