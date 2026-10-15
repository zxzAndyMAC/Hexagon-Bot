// 统计四卡（components E12 stat，usage#detail）：总 Token（较上一周期）· 今日 Token（输入 / 输出占比）·
// 活跃 Agent（休眠数）· 预算（唯一钱口径 + A12 进度 +「设置上限」就地编辑）。网格 4 列 gap 12。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { UsageBucket, UsageRow } from '../../api'
import { useUiStore } from '../../store'
import { LimitEditor, UsageUncertainty } from '../UsageTab'
import { Delta, StatCard } from './parts'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import { capReached, centsToMc, daysAgo, fmtTok, fmtYuan, pctChange, sumTokens, tokenShare } from '../../usage'

export function UsageStats({ series, prevSeries, rows }: { series: UsageBucket[]; prevSeries: UsageBucket[] | null; rows: UsageRow[] }) {
  const { t } = useTranslation()
  const total = useUiStore((s) => s.usageTotal)
  const team = useUiStore((s) => s.team)
  const series7d = useUiStore((s) => s.usageSeries7d)
  const [editing, setEditing] = useState(false)

  const rangeTok = sumTokens(series)
  const delta = prevSeries ? pctChange(rangeTok, sumTokens(prevSeries)) : null
  const calls = rows.reduce((s, r) => s + (r.calls ?? 0), 0)
  // 今日固定取 store 的近 7 日序列（UTC 日界），与所选范围无关
  const today = series7d.filter((b) => b.bucket.startsWith(daysAgo(0)))
  const share = tokenShare(today)
  const sleeping = team.filter((m) => m.status === 'sleeping').length
  const spent = total?.spent_mc ?? 0
  const limitMc = total?.limit_cents != null ? centsToMc(total.limit_cents) : null
  const capped = capReached(spent, total?.limit_cents)

  return (
    <div data-usage-stats className="grid grid-cols-4 gap-3">
      <StatCard data-stat="total" k={t('usageDetail.statTotal')} v={fmtTok(rangeTok)}
        sub={delta != null
          ? <><Delta pct={delta} /> {t('usageDetail.vsPrev')}</>
          : t('usageDetail.calls', { count: calls })} />
      <StatCard data-stat="today" k={t('usage.todayTokens')} v={fmtTok(sumTokens(today))}
        sub={share ? t('usageDetail.share', share) : t('usageDetail.noneToday')} />
      <StatCard data-stat="agents" k={t('usage.activeAgents')}
        v={<>{team.length - sleeping} <span className="text-[13px] font-medium text-ink-3">/ {team.length}</span></>}
        sub={t('usageDetail.sleeping', { count: sleeping })} />
      <StatCard data-stat="budget" k={t('usage.budget')}
        action={!editing && <Button size="xs" variant="ghost" onClick={() => setEditing(true)}>{t('usage.setLimit')}</Button>}
        v={<>
          <span className={capped ? 'text-red' : undefined}>{total ? fmtYuan(spent) : '—'}</span>
          {limitMc != null && <span className="text-[13px] font-medium text-ink-3"> / {fmtYuan(limitMc)}</span>}
        </>}
      >
        {limitMc != null && <Progress className="mt-2" value={Math.min(100, (spent / limitMc) * 100)} tone={capped ? 'cap' : 'accent'}
          aria-label={t('usage.budget')} />}
        {(total?.reserved_mc ?? 0) > 0 && <div className="mt-1.5 text-[12px] text-ink-3" role="status">
          {t('usage.reservedEstimate')} · {fmtYuan(total!.reserved_mc)}
        </div>}
        <UsageUncertainty />
        {editing && <LimitEditor onDone={() => setEditing(false)} />}
      </StatCard>
    </div>
  )
}
