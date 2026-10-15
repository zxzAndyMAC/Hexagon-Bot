// 消费榜单与按阶段（components E12 Leaderboard，usage#detail）。
// 团队视图：所选范围内按 Agent（usage_series 带 agent_id），头像 18 + 名 / 8 高条（Agent 确定色）/ 等宽数值。
// 模型视图：环形图（Pie Donut，中心总量）。按阶段：「已完成 3 / 6」+ 同形行，条色 chart-1，「未归属阶段」名 ink-3、条 chart-3
//（票 12 按原型 usage#detail 改正 E12 原写的 --ink：深色主题下白条过亮，规范同步改）。
// 模型、阶段也按所选范围（2026-10-14 负责人裁决：加 IPC usage_rows(from,to)；原先只有 usage_summary 的项目累计，被否）。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Cell, Label, Pie, PieChart } from 'recharts'
import type { UsageBucket, UsageRow } from '../../api'
import { useUiStore } from '../../store'
import { agentVar } from '../../colors'
import { ChartCard, HBar, UsageEmpty } from './parts'
import { Avatar } from '@/components/ui/avatar'
import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from '@/components/ui/chart'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { fmtTok, groupTokens, stageProgress } from '../../usage'

type Board = 'team' | 'model'

/** 模型环形图用色：chart-1..3，其余并成「其他」走 --line-strong。 */
const DONUT = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)']
const NO_STAGE = '\u0000'

export function Leaderboard({ series, rows }: { series: UsageBucket[]; rows: UsageRow[] }) {
  const { t } = useTranslation()
  const [board, setBoard] = useState<Board>('team')
  const team = useUiStore((s) => s.team)
  const avatars = useUiStore((s) => s.avatars)
  const roleOf = (id: string) => team.find((m) => m.id === id)?.role ?? id

  const byAgent = groupTokens(series, (r) => r.agent_id ?? '—')
  const top = byAgent[0]?.[1] || 1
  return (
    <ChartCard
      data-usage-board={board}
      title={t('usage.leaderboard')}
      desc={t(board === 'team' ? 'usageDetail.boardTeamDesc' : 'usageDetail.boardModelDesc')}
      right={
        <Segmented value={board} onValueChange={(v) => setBoard(v as Board)} aria-label={t('usage.leaderboard')}>
          <SegmentedItem value="team">{t('usageDetail.segTeam')}</SegmentedItem>
          <SegmentedItem value="model">{t('usageDetail.segModel')}</SegmentedItem>
        </Segmented>
      }
    >
      <div className="px-[18px] pt-3 pb-4">
        {board === 'team'
          ? byAgent.length === 0
            ? <UsageEmpty />
            : byAgent.map(([id, tok]) => (
              <HBar key={id}
                label={<><Avatar agentId={id} name={roleOf(id)} size={18} src={avatars[id]} /><span className="truncate">{roleOf(id)}</span></>}
                value={fmtTok(tok)} exact={`${roleOf(id)} · ${tok.toLocaleString()}`}
                max={(tok / top) * 100} color={agentVar(id)} />
            ))
          : <ModelDonut rows={rows} />}
      </div>
    </ChartCard>
  )
}

function ModelDonut({ rows }: { rows: { model?: string | null; prompt_tokens?: number; completion_tokens?: number; tool_output_tokens?: number }[] }) {
  const { t } = useTranslation()
  const byModel = groupTokens(rows, (r) => r.model || '—')
  const total = byModel.reduce((s, [, v]) => s + v, 0)
  if (total === 0) return <UsageEmpty />
  const head = byModel.slice(0, DONUT.length)
  const rest = byModel.slice(DONUT.length).reduce((s, [, v]) => s + v, 0)
  const data = [
    ...head.map(([name, value], i) => ({ key: `m${i}`, name, value, fill: DONUT[i] })),
    ...(rest > 0 ? [{ key: 'other', name: t('usageDetail.otherModels'), value: rest, fill: 'var(--line-strong)' }] : []),
  ]
  const config: ChartConfig = Object.fromEntries(data.map((d) => [d.key, { label: d.name, color: d.fill }]))
  const aria = t('usageDetail.donutAria', {
    total: fmtTok(total),
    parts: data.map((d) => `${d.name} ${Math.round((d.value / total) * 100)}%`).join(', '),
  })
  return (
    <div data-model-donut className="flex items-center gap-4">
      <ChartContainer config={config} className="size-[150px] flex-none" initialDimension={{ width: 150, height: 150 }}>
        <PieChart role="img" aria-label={aria}>
          <ChartTooltip content={<ChartTooltipContent hideLabel valueFormatter={fmtTok} />} />
          <Pie data={data} dataKey="value" nameKey="key" innerRadius={46} outerRadius={68} strokeWidth={2} stroke="var(--surface)" isAnimationActive={false}>
            {data.map((d) => <Cell key={d.key} fill={d.fill} />)}
            <Label content={({ viewBox }) => {
              const vb = viewBox as { cx?: number; cy?: number } | undefined
              if (vb?.cx == null || vb.cy == null) return null
              return (
                <text x={vb.cx} y={vb.cy} textAnchor="middle" dominantBaseline="middle">
                  <tspan x={vb.cx} y={vb.cy - 6} className="fill-ink font-mono text-[15px] font-semibold">{fmtTok(total)}</tspan>
                  <tspan x={vb.cx} y={vb.cy + 12} className="fill-ink-3 text-[11px]">{t('usageDetail.donutCenter')}</tspan>
                </text>
              )
            }} />
          </Pie>
        </PieChart>
      </ChartContainer>
      <ul className="min-w-0 flex-1 text-[12.5px]">
        {data.map((d) => (
          <li key={d.key} className="flex h-7 items-center gap-2">
            <i aria-hidden className="size-2 shrink-0 rounded-[2px]" style={{ background: d.fill }} />
            <span className="min-w-0 flex-1 truncate text-ink-2" title={d.name}>{d.name}</span>
            <span className="font-mono text-[12px] text-ink tabular-nums">{fmtTok(d.value)}</span>
          </li>
        ))}
      </ul>
    </div>
  )
}

export function StageBreakdown({ rows }: { rows: UsageRow[] }) {
  const { t } = useTranslation()
  const stages = useUiStore((s) => s.stages)
  const byStage = groupTokens(rows, (r) => r.stage || NO_STAGE)
  const { done, total } = stageProgress(stages)
  const top = byStage[0]?.[1] || 1
  return (
    <ChartCard data-usage-stages title={t('usage.byStage')} desc={t('usageDetail.stageDesc', { done, total })}>
      <div className="px-[18px] pt-3 pb-4">
        {byStage.length === 0 ? <UsageEmpty /> : byStage.map(([stage, tok]) => {
          const none = stage === NO_STAGE
          const name = none ? t('usageDetail.noStage') : stage
          return (
            <HBar key={stage} label={<span className="truncate">{name}</span>} muted={none}
              value={fmtTok(tok)} exact={`${name} · ${tok.toLocaleString()}`}
              max={(tok / top) * 100} color={none ? 'var(--chart-3)' : 'var(--chart-1)'} />
          )
        })}
      </div>
    </ChartCard>
  )
}
