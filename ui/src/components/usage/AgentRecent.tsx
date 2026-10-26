// 每 Agent 最近使用（usage#agents，components E13 不堆叠形）：同一 Area Chart 形制，每条线用 Agent 确定色
// （foundations §1.5 c1–c8，agentSwatch 按 id 取号，不写死），只填一层很淡的渐变；A7「7 天 · 14 天 · 30 天」，前 4 名。
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type UsageBucket } from '../../api'
import { useUiStore } from '../../store'
import { agentVar } from '../../colors'
import { AreaTrend } from './AreaTrend'
import { ChartCard, ChartSkeleton, UsageEmpty, UsageLoadError } from './parts'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { bucketTick, dayAxis, daysAgo, fmtTok, topAgentRows, usageActorName } from '../../usage'

const TOP_N = 4
const SPANS = [7, 14, 30] as const

export function AgentRecent({ refreshKey }: { refreshKey: number }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const [days, setDays] = useState<(typeof SPANS)[number]>(14)
  const [series, setSeries] = useState<UsageBucket[]>([])
  const [status, setStatus] = useState<'loading' | 'ok' | 'error'>('loading')
  const req = useRef(0)

  const load = useCallback(async () => {
    const id = ++req.current
    try {
      const rows = await api.usageSeries('day', daysAgo(days - 1), daysAgo(0))
      if (id === req.current) { setSeries(rows); setStatus('ok') }
    } catch {
      if (id === req.current) setStatus('error')
    }
  }, [days])
  useEffect(() => { void load() }, [load, refreshKey])

  const roleOf = (id: string) => usageActorName(team, id)
  const { agents, rows } = topAgentRows(series, dayAxis(daysAgo(days - 1), daysAgo(0)), TOP_N)
  const lines = agents.map((id) => ({ key: id, label: roleOf(id), color: agentVar(id) }))
  const aria = t('usageDetail.agentsAria', {
    days,
    parts: agents.map((id) => `${roleOf(id)} ${fmtTok(rows.reduce((s, r) => s + Number(r[id] ?? 0), 0))}`).join(', '),
  })

  return (
    <ChartCard
      data-usage-agents
      title={t('usage.recentTop')}
      desc={t('usageDetail.agentsDesc', { days, n: TOP_N })}
      right={
        <Segmented value={String(days)} onValueChange={(v) => setDays(Number(v) as (typeof SPANS)[number])} aria-label={t('usage.range')}>
          {SPANS.map((d) => <SegmentedItem key={d} value={String(d)}>{t('usageDetail.days', { count: d })}</SegmentedItem>)}
        </Segmented>
      }
    >
      <div className="px-3 pt-4 pb-1">
        {status === 'loading' ? <ChartSkeleton height={200} />
          : status === 'error' ? <div className="px-1.5 pb-3"><UsageLoadError onRetry={() => { setStatus('loading'); void load() }} /></div>
          : agents.length === 0 ? <UsageEmpty />
          : <AreaTrend stacked={false} height={200} data={rows} series={lines} ariaLabel={aria}
              tick={(b) => bucketTick(b, 'day')} />}
      </div>
    </ChartCard>
  )
}
