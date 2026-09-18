import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import * as echarts from 'echarts/core'
import { BarChart, LineChart, PieChart } from 'echarts/charts'
import { GridComponent, LegendComponent, TooltipComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import type { EChartsCoreOption } from 'echarts/core'
import { api, type UsageBucket } from '../api'
import { useUiStore } from '../store'
import { agentColor } from '../colors'
import {
  breakdownRows, capReached, centsToMc, fmtTok, fmtYuan,
  groupTokens, parseLimitInput, perAgentSeries, tokenTypeSeries, tokensOf,
} from '../usage'

echarts.use([LineChart, PieChart, BarChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer])

const cssVar = (name: string) =>
  getComputedStyle(document.documentElement).getPropertyValue(name).trim()

type Granularity = 'day' | 'hour'
const RANGES = [7, 14, 30, 0] as const // 0 = 全部
type Board = 'model' | 'team'

export function UsageDetailTab() {
  const { t } = useTranslation()
  const { usageRows, usageTotal, team, themePref, refresh } = useUiStore()
  const [granularity, setGranularity] = useState<Granularity>('day')
  const [days, setDays] = useState<number>(30)
  const [board, setBoard] = useState<Board>('team')
  const [series, setSeries] = useState<UsageBucket[]>([])
  const [editLimit, setEditLimit] = useState('')
  const [editing, setEditing] = useState(false)

  const load = useCallback(() => {
    api.usageSeries(granularity, days || null).then(setSeries).catch(() => setSeries([]))
  }, [granularity, days])

  useEffect(load, [load])

  const rows = breakdownRows(usageRows)
  const roleOf = (id?: string | null) => team.find((m) => m.id === id)?.role ?? id ?? '—'
  const colorOf = (id: string) => agentColor(id)

  const spent = usageTotal?.spent_mc ?? 0
  const limitMc = usageTotal?.limit_cents != null ? centsToMc(usageTotal.limit_cents) : null
  const capped = capReached(spent, usageTotal?.limit_cents)
  const totalCalls = rows.reduce((s, r) => s + (r.calls ?? 0), 0)
  const totalTok = usageTotal?.tokens ?? rows.reduce((s, r) => s + tokensOfRow(r), 0)
  const today = new Date().toISOString().slice(0, 10)
  const todayTok = series.filter((b) => b.bucket.startsWith(today)).reduce((s, b) => s + tokensOf(b), 0)

  const byModel = groupTokens(rows, (r) => r.model || '—')
  const byAgent = groupTokens(rows, (r) => r.agent_id ?? '—')
  const byStage = groupTokens(rows, (r) => r.stage || t('usage.noStage'))

  const trend = useMemo(() => tokenTypeSeries(series), [series])
  const perAgent = useMemo(() => perAgentSeries(series), [series])

  // 主题变量换肤后重建 option
  void themePref
  const ax = { axisLine: { lineStyle: { color: cssVar('--border') } }, axisLabel: { color: cssVar('--text-3'), fontSize: 10 }, splitLine: { lineStyle: { color: cssVar('--bg-3') || 'rgba(128,128,128,.15)' } } }
  const tip = { trigger: 'axis' as const, textStyle: { fontSize: 11 }, backgroundColor: cssVar('--bg-1'), borderColor: cssVar('--border'), valueFormatter: (v: number) => fmtTok(v) }
  const short = (b: string) => (granularity === 'hour' ? b.slice(11) : b.slice(5))

  const boardOption: EChartsCoreOption = board === 'model'
    ? {
        tooltip: { trigger: 'item', valueFormatter: (v: number) => fmtTok(v) },
        legend: { textStyle: { color: cssVar('--text-2'), fontSize: 11 }, bottom: 0 },
        series: [{
          type: 'pie', radius: ['45%', '70%'], center: ['50%', '44%'],
          label: { color: cssVar('--text-2'), fontSize: 11, formatter: '{b}\n{d}%' },
          data: byModel.map(([name, cost]) => ({ name, value: cost })),
        }],
      }
    : {
        tooltip: { ...tip, trigger: 'item' },
        grid: { left: 8, right: 24, top: 8, bottom: 8, containLabel: true },
        xAxis: { type: 'value', ...ax, axisLabel: { ...ax.axisLabel, formatter: (v: number) => fmtTok(v) } },
        yAxis: { type: 'category', data: byAgent.map(([id]) => roleOf(id)).reverse(), ...ax },
        series: [{
          type: 'bar', barWidth: 12,
          data: byAgent.map(([id, cost]) => ({ value: cost, itemStyle: { color: colorOf(id) } })).reverse(),
          label: { show: true, position: 'right', color: cssVar('--text-3'), fontSize: 10, formatter: (p: { value: number }) => fmtTok(p.value) },
        }],
      }

  const trendOption: EChartsCoreOption = {
    tooltip: tip,
    legend: { textStyle: { color: cssVar('--text-2'), fontSize: 11 }, top: 0 },
    grid: { left: 8, right: 12, top: 28, bottom: 8, containLabel: true },
    xAxis: { type: 'category', data: trend.labels.map(short), ...ax },
    yAxis: { type: 'value', ...ax, axisLabel: { ...ax.axisLabel, formatter: (v: number) => fmtTok(v) } },
    series: ([t('usage.tokPrompt'), t('usage.tokCompletion'), t('usage.tokToolOutput')] as const).map((name, i) => ({
      name, type: 'line', smooth: true, showSymbol: false, data: trend.series[i],
      lineStyle: { width: 1.5 },
    })),
  }

  const agentOption: EChartsCoreOption = {
    tooltip: tip,
    legend: { textStyle: { color: cssVar('--text-2'), fontSize: 11 }, top: 0 },
    grid: { left: 8, right: 12, top: 28, bottom: 8, containLabel: true },
    xAxis: { type: 'category', data: perAgent.labels.map(short), ...ax },
    yAxis: { type: 'value', ...ax, axisLabel: { ...ax.axisLabel, formatter: (v: number) => fmtTok(v) } },
    series: perAgent.series.map((s) => ({
      name: roleOf(s.agentId), type: 'line', smooth: true, showSymbol: false,
      data: s.points, lineStyle: { width: 1.5 }, itemStyle: { color: colorOf(s.agentId) },
    })),
  }

  return (
    <div style={{ flex: 1, overflowY: 'auto', padding: '12px 16px', fontSize: 12, display: 'flex', flexDirection: 'column', gap: 14 }}>
      {/* 总览卡 */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 10 }}>
        <Stat label={t('usage.totalTokens')} value={fmtTok(totalTok)} />
        <Stat label={t('usage.totalCalls')} value={String(totalCalls)} />
        <Stat label={t('usage.todayTokens')} value={fmtTok(todayTok)} />
        <Stat
          label={t('topbar.usage')}
          value={`${fmtYuan(spent)}${limitMc != null ? ` / ${fmtYuan(limitMc)}` : ''}`}
          alert={capped ? t('usage.capHit') : undefined}
        />
      </div>
      {capped && <div className="chip err" style={{ padding: '6px 10px' }}>⚠ {t('usage.capHit')}</div>}

      {/* 筛选条 */}
      <div className="row-line" style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '6px 10px' }}>
        <span className="dim3" style={{ fontSize: 10 }}>{t('usage.granularity')}</span>
        <Seg value={granularity} onChange={(v) => setGranularity(v as Granularity)}
          options={[['day', t('usage.gDay')], ['hour', t('usage.gHour')]]} />
        <span className="dim3" style={{ fontSize: 10, marginLeft: 8 }}>{t('usage.range')}</span>
        <Seg value={String(days)} onChange={(v) => setDays(Number(v))}
          options={RANGES.map((d) => [String(d), d === 0 ? t('usage.rAll') : t('usage.rDays', { n: d })])} />
        <div style={{ flex: 1 }} />
        <button className="btn" style={{ fontSize: 11 }} onClick={async () => { load(); await refresh() }}>
          {t('usage.refresh')}
        </button>
      </div>

      {/* 消费榜单：模型分布 ⇄ 团队榜 */}
      <Card title={t('usage.leaderboard')}
        extra={<Seg value={board} onChange={(v) => setBoard(v as Board)}
          options={[['team', t('usage.teamRank')], ['model', t('usage.modelDist')]]} />}>
        <Chart option={boardOption} height={200} />
      </Card>

      {/* token 使用趋势（三类 token） */}
      <Card title={t('usage.tokenTrend')}>
        <Chart option={trendOption} height={220} />
      </Card>

      {/* 最近使用：按 Agent 日 token */}
      <Card title={t('usage.recentTop')}>
        <Chart option={agentOption} height={220} />
      </Card>

      {/* 分解 + 上限 */}
      <Card title={t('usage.byStage')}>
        {byStage.map(([stage, cost]) => (
          <div key={stage} style={{ display: 'flex', justifyContent: 'space-between', padding: '3px 0', fontSize: 11 }}>
            <span>{stage}</span><span className="mono dim">{fmtTok(cost)}</span>
          </div>
        ))}
        <div style={{ display: 'flex', gap: 6, marginTop: 10 }}>
          {editing ? (
            <>
              <input value={editLimit} onChange={(e) => setEditLimit(e.target.value)} placeholder={t('usage.limitHint')}
                className="mono" style={{ flex: 1, fontSize: 11, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px', outline: 'none' }} />
              <button className="btn primary" style={{ fontSize: 11 }} onClick={async () => {
                await api.setUsageLimit(parseLimitInput(editLimit)); setEditing(false); await refresh()
              }}>{t('cards.confirm')}</button>
              <button className="btn" style={{ fontSize: 11 }} onClick={() => setEditing(false)}>✕</button>
            </>
          ) : (
            <button className="btn" style={{ fontSize: 11 }} onClick={() => {
              setEditLimit(usageTotal?.limit_cents ? String(usageTotal.limit_cents / 100) : ''); setEditing(true)
            }}>{t('usage.setLimit')}</button>
          )}
        </div>
      </Card>
    </div>
  )
}

function tokensOfRow(r: { prompt_tokens?: number; completion_tokens?: number; tool_output_tokens?: number }) {
  return (r.prompt_tokens ?? 0) + (r.completion_tokens ?? 0) + (r.tool_output_tokens ?? 0)
}

function Stat({ label, value, alert }: { label: string; value: string; alert?: string }) {
  return (
    <div className="row-line" style={{ padding: '10px 12px', borderRadius: 8 }}>
      <div className="dim3" style={{ fontSize: 10, marginBottom: 4 }}>{label}</div>
      <div className="mono" style={{ fontSize: 16, fontWeight: 560 }}>{value}</div>
      {alert && <div className="dim" style={{ fontSize: 10, marginTop: 3, color: 'var(--err)' }}>{alert}</div>}
    </div>
  )
}

function Seg({ value, onChange, options }: {
  value: string
  onChange: (v: string) => void
  options: [string, string][]
}) {
  return (
    <span style={{ display: 'inline-flex', gap: 2 }}>
      {options.map(([v, label]) => (
        <button key={v} className="btn" onClick={() => onChange(v)}
          style={{
            padding: '2px 8px', fontSize: 10, border: 'none',
            background: value === v ? 'var(--accent-soft)' : 'none',
            color: value === v ? 'var(--accent)' : 'var(--text-3)',
            fontWeight: value === v ? 560 : 400,
          }}>{label}</button>
      ))}
    </span>
  )
}

function Card({ title, extra, children }: { title: string; extra?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="row-line" style={{ padding: '10px 12px', borderRadius: 8 }}>
      <div style={{ display: 'flex', alignItems: 'center', marginBottom: 8 }}>
        <span style={{ fontSize: 11, fontWeight: 560 }}>{title}</span>
        <div style={{ flex: 1 }} />
        {extra}
      </div>
      {children}
    </div>
  )
}

function Chart({ option, height }: { option: EChartsCoreOption; height: number }) {
  const ref = useRef<HTMLDivElement>(null)
  const inst = useRef<echarts.ECharts | null>(null)
  useEffect(() => {
    if (!ref.current) return
    inst.current = echarts.init(ref.current)
    const ro = new ResizeObserver(() => inst.current?.resize())
    ro.observe(ref.current)
    return () => { ro.disconnect(); inst.current?.dispose(); inst.current = null }
  }, [])
  useEffect(() => { inst.current?.setOption(option, true) }, [option])
  return <div ref={ref} style={{ width: '100%', height }} />
}
