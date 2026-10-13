// 阶段列表 StageList（components B2，workbench#running）：侧栏「流程」分组下的竖向阶段，只读（ADR 0069）。
// 不提供退回、跳过、盖章按钮；跳转用节点轨，「查看流程」打开只读流程。取代旧右栏「阶段」页签的 StageRail（ADR 0088）。
// 每行 28 高：15px 圆点 + 名 13px + 右侧 meta 11.5 ink-3；行间 1.5px 竖连线（左 6.5），done/active 前的连线绿。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import type { StageRow } from '../../gen/StageRow'
import { Icon } from '../Icon'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

type State = StageRow['state']

/** 领域态 → 状态全称 i18n key（沿用 stage.* 词条）。 */
const STATE_KEY: Record<State, string> = {
  pending: 'stage.pending',
  active: 'stage.active',
  done: 'stage.done',
  skipped: 'stage.skipped',
  waiting_stamp: 'stage.waiting',
  rejected: 'stage.rejected',
  interrupted: 'stage.interrupted',
}

/** 右侧 meta 只给「正在发生」的几态（原型只给进行中）；done / pending / skipped 靠圆点表达。 */
const META: ReadonlySet<State> = new Set(['active', 'waiting_stamp', 'interrupted', 'rejected'])

function StageDot({ state }: { state: State }) {
  const base = 'relative z-[1] grid size-[15px] flex-none place-items-center rounded-full bg-sidebar'
  switch (state) {
    case 'done':
      return <span className={cn(base, 'bg-green text-white')}><Icon name="check" size={10} /></span>
    case 'active':
      return (
        <span className={cn(base, 'shadow-[inset_0_0_0_1.5px_var(--orange)]')}>
          <span className="size-[5px] animate-hx-pulse rounded-full bg-orange" />
        </span>
      )
    case 'waiting_stamp':
      return <span className={cn(base, 'text-orange shadow-[inset_0_0_0_1.5px_var(--orange)]')}><Icon name="stamp" size={9} /></span>
    case 'interrupted':
      return <span className={cn(base, 'text-orange shadow-[inset_0_0_0_1.5px_var(--orange)]')}><Icon name="warn" size={9} /></span>
    case 'rejected':
      return <span className={cn(base, 'text-red shadow-[inset_0_0_0_1.5px_var(--red)]')}><Icon name="close" size={9} /></span>
    default:
      return <span className={cn(base, 'shadow-[inset_0_0_0_1.5px_var(--line-strong)]')} />
  }
}

export function StageList() {
  const { t } = useTranslation()
  const stages = useUiStore((s) => s.stages)
  const artifacts = useUiStore((s) => s.artifacts)
  const sorted = [...stages].sort((a, b) => a.seq - b.seq)
  if (sorted.length === 0) {
    return <div className="px-[18px] py-1 text-[12px] text-ink-3">{t('sidebar.noStages')}</div>
  }
  return (
    <ul data-stage-list aria-label={t('sidebar.flow')} className="pr-2.5 pl-[18px] pt-0.5">
      {sorted.map((s, i) => {
        const state = t(STATE_KEY[s.state])
        const count = artifacts.filter((a) => a.stage_run_id === s.run_id).length
        const prevGreen = i > 0 && (s.state === 'done' || s.state === 'active')
        return (
          <Tooltip key={s.run_id}>
            <TooltipTrigger asChild>
              <li
                data-stage-state={s.state}
                aria-label={`${s.stage} · ${state}`}
                className={cn(
                  'relative flex h-7 items-center gap-2.5 text-[13px] text-ink-2',
                  s.state === 'active' && 'font-medium text-ink',
                  s.state === 'skipped' && 'text-ink-3',
                )}
              >
                {i > 0 && (
                  <span aria-hidden className={cn('absolute top-[-7px] left-[6.5px] h-3.5 w-[1.5px]', prevGreen ? 'bg-green' : 'bg-line-strong')} />
                )}
                <StageDot state={s.state} />
                <span className="min-w-0 truncate">{s.stage}</span>
                {META.has(s.state) && <span className="ml-auto shrink-0 text-[11.5px] font-normal text-ink-3">{state}</span>}
              </li>
            </TooltipTrigger>
            <TooltipContent side="right">{t('sidebar.stageTip', { stage: s.stage, state, count })}</TooltipContent>
          </Tooltip>
        )
      })}
    </ul>
  )
}
