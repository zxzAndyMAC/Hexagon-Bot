// 用量详情主区视图（interaction §3 §10，usage#detail；ADR 0090）：占满主区，对话与输入条一起让出，侧栏仍在。
// 票 03 把旧用量详情从中栏页签挪成主区视图；票 12 换皮：头部 crumb + 粒度 A7 + 时间范围 A23 + 刷新；
// 统计四卡 E12 → Token 趋势 E13（三层堆叠）→ 消费榜单 / 按阶段 → 每 Agent 最近使用。日期按 UTC 日界对齐后端分桶。
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type UsageBucket } from '../../api'
import { useUiStore } from '../../store'
import { isMac } from '../../keymap'
import { Icon } from '../Icon'
import { AreaTrend } from '../usage/AreaTrend'
import { AgentRecent } from '../usage/AgentRecent'
import { RangePicker } from '../usage/RangePicker'
import { Leaderboard, StageBreakdown } from '../usage/UsageBoards'
import { UsageStats } from '../usage/UsageStats'
import { ChartCard, ChartSkeleton, Delta, UsageEmpty, UsageLoadError } from '../usage/parts'
import { Button } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'
import {
  bucketTick, dayAxis, rangeLabel, daysAgo, fmtTok, presetRange, previousRange, stackRows, sumTokens, weekOverWeek,
  type UsageRange,
} from '../../usage'

type Granularity = 'day' | 'hour'
type Status = 'loading' | 'ok' | 'error'

/** 趋势卡尾「较上周」只在区间 ≥ 7 天（或全部时间）时有意义。 */
const spansWeek = (r: UsageRange) =>
  r.from == null || r.to == null || Date.parse(r.to) - Date.parse(r.from) >= 6 * 864e5

export function UsageView({ narrowLead = false }: { narrowLead?: boolean }) {
  const { t } = useTranslation()
  const projectName = useUiStore((s) => s.projectName)
  const [granularity, setGranularity] = useState<Granularity>('day')
  const [range, setRange] = useState<UsageRange>(() => presetRange('30d'))
  const [series, setSeries] = useState<UsageBucket[]>([])
  const [prevSeries, setPrevSeries] = useState<UsageBucket[] | null>(null)
  const [status, setStatus] = useState<Status>('loading')
  const [refreshing, setRefreshing] = useState(false)
  const [refreshKey, setRefreshKey] = useState(0)
  const req = useRef(0)

  const load = useCallback(async () => {
    const id = ++req.current
    const prev = previousRange(range)
    try {
      const [cur, before] = await Promise.all([
        api.usageSeries(granularity, range.from, range.to),
        prev ? api.usageSeries('day', prev.from, prev.to) : Promise.resolve(null),
      ])
      if (id !== req.current) return
      setSeries(cur)
      setPrevSeries(before)
      setStatus('ok')
    } catch {
      if (id === req.current) setStatus('error')
    }
  }, [granularity, range])
  useEffect(() => { void load() }, [load])

  const refresh = async () => {
    setRefreshing(true)
    setRefreshKey((k) => k + 1)
    try {
      await Promise.all([load(), useUiStore.getState().invalidate('usage')])
    } finally {
      setRefreshing(false)
    }
  }

  const label = rangeLabel(range, t)
  const to = range.to ?? daysAgo(0)
  const first = series[0]?.bucket.slice(0, 10)
  const axis = granularity === 'day' ? dayAxis(range.from ?? first ?? to, to) : undefined
  const rows = stackRows(series, axis)
  const total = sumTokens(series)
  const wow = spansWeek(range) ? weekOverWeek([...(prevSeries ?? []), ...series], to) : null
  const layers = [
    { key: 'tool', label: t('usage.tokToolOutput'), color: 'var(--chart-3)' },
    { key: 'output', label: t('usage.tokCompletion'), color: 'var(--chart-2)' },
    { key: 'input', label: t('usage.tokPrompt'), color: 'var(--chart-1)' },
  ]
  const sum = (k: 'input' | 'output' | 'tool') => fmtTok(rows.reduce((s, r) => s + r[k], 0))
  const trendAria = t('usageDetail.trendAria', { range: label, total: fmtTok(total), input: sum('input'), output: sum('output'), tool: sum('tool') })
  const span = range.from ? `${range.from} – ${to}` : t('usage.p_all')

  return (
    <section data-usage-view aria-label={t('usage.detail')} className="flex min-h-0 min-w-0 flex-1 flex-col">
      <header data-tauri-drag-region className={cn('flex h-12 flex-none items-center gap-2 border-b border-line-soft pr-3 pl-4', isMac && narrowLead && 'pl-7')}>
        <Button variant="ghost" size="xs" data-usage-back onClick={() => useUiStore.getState().showConversation()}>
          <Icon name="arrow-left" size={14} />{t('canvas.backToChat')}
        </Button>
        <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center gap-1.5 text-[13px] text-ink-2">
          <b className="font-semibold text-ink">{t('usageEntry.labelPlain')}</b>
          {projectName && <span className="truncate text-ink-3">{projectName}</span>}
        </div>
        <Segmented value={granularity} onValueChange={(v) => setGranularity(v as Granularity)} aria-label={t('usage.granularity')} data-usage-granularity>
          <SegmentedItem value="day">{t('usage.gDay')}</SegmentedItem>
          <SegmentedItem value="hour">{t('usage.gHour')}</SegmentedItem>
        </Segmented>
        <RangePicker range={range} onChange={setRange} data-usage-range="header" />
        <IconButton label={t('usage.refresh')} loading={refreshing} onClick={() => void refresh()} data-usage-refresh>
          <Icon name="refresh" size={14} />
        </IconButton>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
        <div className="mx-auto flex max-w-[1080px] flex-col gap-4">
          <UsageStats series={series} prevSeries={prevSeries} />
          <ChartCard
            data-usage-trend
            title={t('usage.tokenTrend')}
            desc={t('usageDetail.trendDesc', { range: label, granularity: t(granularity === 'day' ? 'usage.gDay' : 'usage.gHour') })}
            right={<RangePicker range={range} onChange={setRange} data-usage-range="chart" />}
            footer={<>
              {wow != null && <b className="inline-flex items-center gap-1.5 font-medium text-ink">
                {t('usageDetail.vsLastWeek')} <Delta pct={wow} />
                <Icon name={wow >= 0 ? 'trend' : 'trend-down'} size={14} className={wow >= 0 ? 'text-green' : 'text-ink-2'} />
              </b>}
              <span className="block text-ink-3">{t('usageDetail.utcNote', { span })}</span>
            </>}
          >
            <div className="px-3 pt-4 pb-1">
              {status === 'loading' ? <ChartSkeleton height={230} />
                : status === 'error' ? <div className="px-1.5 pb-3"><UsageLoadError onRetry={() => { setStatus('loading'); void load() }} /></div>
                : total === 0 ? <UsageEmpty
                    actionLabel={range.preset === 'all' ? undefined : t('usage.p_all')}
                    onAction={range.preset === 'all' ? undefined : () => setRange(presetRange('all'))} />
                : <AreaTrend stacked height={230} data={rows} series={layers} ariaLabel={trendAria}
                    tick={(b) => bucketTick(b, granularity)} tooltipLabel={(b) => b} />}
            </div>
          </ChartCard>
          <div className="grid grid-cols-2 gap-3">
            <Leaderboard series={series} />
            <StageBreakdown />
          </div>
          <AgentRecent refreshKey={refreshKey} />
        </div>
      </div>
    </section>
  )
}
