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
  groupTokens, parseLimitInput, perAgentSeries, tokenTypeSeries,
} from '../usage'

echarts.use([LineChart, PieChart, BarChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer])

const cssVar = (name: string) =>
  getComputedStyle(document.documentElement).getPropertyValue(name).trim()

type Granularity = 'day' | 'hour'
const RANGES = [7, 14, 30, 0] as const // 0 = 全部
type Board = 'team' | 'model'

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

  const spent = usageTotal?.spent_mc ?? 0
  const limitMc = usageTotal?.limit_cents != null ? centsToMc(usageTotal.limit_cents) : null
  const capped = capReached(spent, usageTotal?.limit_cents)
  const pct = limitMc ? Math.min(100, (spent / limitMc) * 100) : 0
  const totalCalls = rows.reduce((s, r) => s + (r.calls ?? 0), 0)
  const totalTok = usageTotal?.tokens ?? 0
  const today = new Date().toISOString().slice(0, 10)
  const todayTok = series.filter((b) => b.bucket.startsWith(today)).reduce((s, b) => s + tokOf(b), 0)

  const byModel = groupTokens(rows, (r) => r.model || '—')
  const byAgent = groupTokens(rows, (r) => r.agent_id ?? '—')
  const byStage = groupTokens(rows, (r) => r.stage || t('usage.noStage'))
  const tokSum = Math.max(1, byModel.reduce((s, [, v]) => s + v, 0))

  const trend = useMemo(() => tokenTypeSeries(series), [series])
  const perAgent = useMemo(() => perAgentSeries(series), [series])

  // 主题变量换肤后重建 option
  void themePref
  const text2 = cssVar('--text-2')
  const text3 = cssVar('--text-3')
  const axis = {
    axisLine: { lineStyle: { color: cssVar('--border') } },
    axisTick: { show: false },
    axisLabel: { color: text3, fontSize: 10 },
    splitLine: { lineStyle: { color: cssVar('--border'), type: 'dashed' as const } },
  }
  const tip = {
    trigger: 'axis' as const,
    backgroundColor: cssVar('--bg-1'), borderColor: cssVar('--border'),
    textStyle: { color: cssVar('--text'), fontSize: 11 },
    valueFormatter: (v: number) => fmtTok(v),
  }
  const legend = { textStyle: { color: text2, fontSize: 11 }, itemWidth: 14, itemHeight: 8, icon: 'roundRect' }
  const grid = { left: 8, right: 16, top: 32, bottom: 4, containLabel: true }
  const palette = [cssVar('--accent'), cssVar('--ok'), cssVar('--flag'), cssVar('--err')]
  const short = (b: string) => (granularity === 'hour' ? b.slice(11) : b.slice(5))

  const boardOption: EChartsCoreOption = board === 'model'
    ? {
        color: palette,
        tooltip: { ...tip, trigger: 'item' },
        legend: { ...legend, orient: 'vertical', right: 8, top: 'middle',
          formatter: (name: string) => `${name}  ${fmtTok(byModel.find(([n]) => n === name)?.[1] ?? 0)}` },
        series: [{
          type: 'pie', radius: ['52%', '74%'], center: ['34%', '50%'],
          itemStyle: { borderColor: cssVar('--bg-1'), borderWidth: 2 },
          label: { show: false },
          emphasis: { label: { show: true, fontSize: 12, color: text2, formatter: '{b}\n{d}%' } },
          data: byModel.map(([name, v]) => ({ name, value: v })),
        }],
        graphic: [{
          type: 'text', left: '29%', top: '46%',
          style: { text: fmtTok(tokSum === 1 ? 0 : tokSum), fontSize: 16, fontWeight: 560, fill: cssVar('--text'), fontFamily: 'ui-monospace, Menlo, monospace', textAlign: 'center' },
        }],
      }
    : {
        tooltip: { ...tip, trigger: 'item' },
        grid: { left: 8, right: 48, top: 8, bottom: 4, containLabel: true },
        xAxis: { type: 'value', ...axis, axisLabel: { ...axis.axisLabel, formatter: (v: number) => fmtTok(v) }, splitLine: { show: false } },
        yAxis: { type: 'category', data: byAgent.map(([id]) => roleOf(id)).reverse(), ...axis, splitLine: { show: false } },
        series: [{
          type: 'bar', barWidth: 14,
          itemStyle: { borderRadius: [0, 4, 4, 0] },
          data: byAgent.map(([id, v]) => ({ value: v, itemStyle: { color: agentColor(id, 50) } })).reverse(),
          label: { show: true, position: 'right', color: text3, fontSize: 10, fontFamily: 'ui-monospace, Menlo, monospace', formatter: (p: { value: number }) => fmtTok(p.value) },
        }],
      }

  const lineBase = { type: 'line' as const, smooth: true, showSymbol: false, symbolSize: 5, lineStyle: { width: 1.5 } }
  const trendOption: EChartsCoreOption = {
    color: palette,
    tooltip: tip, legend: { ...legend, top: 0 }, grid,
    xAxis: { type: 'category', boundaryGap: false, data: trend.labels.map(short), ...axis, splitLine: { show: false } },
    yAxis: { type: 'value', ...axis, axisLabel: { ...axis.axisLabel, formatter: (v: number) => fmtTok(v) } },
    series: ([t('usage.tokPrompt'), t('usage.tokCompletion'), t('usage.tokToolOutput')] as const).map((name, i) => ({
      name, ...lineBase, data: trend.series[i],
    })),
  }

  const agentOption: EChartsCoreOption = {
    tooltip: tip, legend: { ...legend, top: 0 }, grid,
    xAxis: { type: 'category', boundaryGap: false, data: perAgent.labels.map(short), ...axis, splitLine: { show: false } },
    yAxis: { type: 'value', ...axis, axisLabel: { ...axis.axisLabel, formatter: (v: number) => fmtTok(v) } },
    series: perAgent.series.map((s) => ({
      name: roleOf(s.agentId), ...lineBase, data: s.points,
      itemStyle: { color: agentColor(s.agentId, 55) },
    })),
  }

  return (
    <div style={{ flex: 1, overflowY: 'auto', padding: '16px 20px', fontSize: 12 }}>
      <div style={{ maxWidth: 880, margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 14 }}>

        {/* 总览卡 */}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12 }}>
          <Stat icon="◔" tint="var(--accent)" tintBg="var(--accent-soft)"
            label={t('usage.totalTokens')} value={fmtTok(totalTok)} sub={`${totalCalls} ${t('usage.calls')}`} />
          <Stat icon="↯" tint="var(--flag)" tintBg="var(--flag-soft)"
            label={t('usage.todayTokens')} value={fmtTok(todayTok)} sub={today} />
          <Stat icon="◍" tint="var(--ok)" tintBg="var(--ok-soft)"
            label={t('usage.activeAgents')}
            value={`${team.filter((m) => m.status === 'active').length}/${team.length}`}
            sub={capped ? t('usage.capHit') : undefined} />
          {/* 预算卡：唯一钱口径 + 进度条 + 上限编辑 */}
          <div className="u-card" style={{ padding: '12px 14px', display: 'flex', gap: 10 }}>
            <span className="stat-ic" style={{ color: capped ? 'var(--err)' : 'var(--accent)', background: capped ? 'var(--err-soft)' : 'var(--accent-soft)' }}>¥</span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="dim3" style={{ fontSize: 10 }}>{t('usage.budget')}</div>
              <div className="mono" style={{ fontSize: 15, fontWeight: 560, marginTop: 2 }}>
                {fmtYuan(spent)}{limitMc != null && <span className="dim3" style={{ fontSize: 12 }}> / {fmtYuan(limitMc)}</span>}
              </div>
              <div style={{ height: 3, borderRadius: 2, background: 'var(--bg-3)', marginTop: 6, overflow: 'hidden' }}>
                <div style={{ width: `${pct}%`, height: '100%', background: capped ? 'var(--err)' : 'var(--accent)' }} />
              </div>
              {editing ? (
                <div style={{ display: 'flex', gap: 4, marginTop: 6 }}>
                  <input value={editLimit} onChange={(e) => setEditLimit(e.target.value)} placeholder={t('usage.limitHint')}
                    className="mono" style={{ flex: 1, minWidth: 0, fontSize: 10, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 5, padding: '2px 6px', outline: 'none' }} />
                  <button className="btn primary" style={{ fontSize: 10, padding: '2px 8px' }} onClick={async () => {
                    await api.setUsageLimit(parseLimitInput(editLimit)); setEditing(false); await refresh()
                  }}>{t('cards.confirm')}</button>
                </div>
              ) : (
                <button className="icon-btn" style={{ fontSize: 10, padding: '2px 4px', marginTop: 4 }}
                  onClick={() => { setEditLimit(usageTotal?.limit_cents ? String(usageTotal.limit_cents / 100) : ''); setEditing(true) }}>
                  {capped ? `⚠ ${t('usage.capHit')}` : t('usage.setLimit')}
                </button>
              )}
            </div>
          </div>
        </div>

        {/* 筛选条 */}
        <div className="u-card" style={{ display: 'flex', alignItems: 'center', gap: 14, padding: '8px 14px' }}>
          <Filter label={t('usage.granularity')}>
            <div className="seg">
              {(['day', 'hour'] as const).map((g) => (
                <button key={g} className={granularity === g ? 'on' : ''} onClick={() => setGranularity(g)}>
                  {t(g === 'day' ? 'usage.gDay' : 'usage.gHour')}
                </button>
              ))}
            </div>
          </Filter>
          <Filter label={t('usage.range')}>
            <div className="seg">
              {RANGES.map((d) => (
                <button key={d} className={days === d ? 'on' : ''} onClick={() => setDays(d)}>
                  {d === 0 ? t('usage.rAll') : t('usage.rDays', { n: d })}
                </button>
              ))}
            </div>
          </Filter>
          <div style={{ flex: 1 }} />
          <button className="btn" style={{ fontSize: 11 }} onClick={async () => { load(); await refresh() }}>
            ⟳ {t('usage.refresh')}
          </button>
        </div>

        {/* 消费榜单 + 阶段分解 */}
        <div style={{ display: 'grid', gridTemplateColumns: '1.4fr 1fr', gap: 14 }}>
          <div className="u-card">
            <div style={{ display: 'flex', alignItems: 'center', marginBottom: 10 }}>
              <span className="u-card-title">{t('usage.leaderboard')}</span>
              <div style={{ flex: 1 }} />
              <div className="seg">
                {(['team', 'model'] as const).map((b) => (
                  <button key={b} className={board === b ? 'on' : ''} onClick={() => setBoard(b)}>
                    {t(b === 'team' ? 'usage.teamRank' : 'usage.modelDist')}
                  </button>
                ))}
              </div>
            </div>
            <Chart option={boardOption} height={210} />
          </div>
          <div className="u-card">
            <div className="u-card-title" style={{ marginBottom: 10 }}>{t('usage.byStage')}</div>
            {byStage.map(([stage, tok]) => (
              <div key={stage} style={{ marginBottom: 10 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, marginBottom: 3 }}>
                  <span>{stage}</span>
                  <span className="mono dim3">{fmtTok(tok)}</span>
                </div>
                <div style={{ height: 4, borderRadius: 2, background: 'var(--bg-3)', overflow: 'hidden' }}>
                  <div style={{ width: `${(tok / (byStage[0]?.[1] || 1)) * 100}%`, height: '100%', background: 'var(--accent)', borderRadius: 2 }} />
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* 趋势两图 */}
        <div className="u-card">
          <div className="u-card-title" style={{ marginBottom: 6 }}>{t('usage.tokenTrend')}</div>
          <Chart option={trendOption} height={230} />
        </div>
        <div className="u-card">
          <div className="u-card-title" style={{ marginBottom: 6 }}>{t('usage.recentTop')}</div>
          <Chart option={agentOption} height={230} />
        </div>
      </div>
    </div>
  )
}

const tokOf = (b: UsageBucket) => b.prompt_tokens + b.completion_tokens + b.tool_output_tokens

function Stat({ icon, tint, tintBg, label, value, sub }: {
  icon: string; tint: string; tintBg: string; label: string; value: string; sub?: string
}) {
  return (
    <div className="u-card" style={{ padding: '12px 14px', display: 'flex', gap: 10 }}>
      <span className="stat-ic" style={{ color: tint, background: tintBg }}>{icon}</span>
      <div style={{ minWidth: 0 }}>
        <div className="dim3" style={{ fontSize: 10 }}>{label}</div>
        <div className="mono" style={{ fontSize: 17, fontWeight: 560, marginTop: 2 }}>{value}</div>
        {sub && <div className="dim3" style={{ fontSize: 10, marginTop: 2 }}>{sub}</div>}
      </div>
    </div>
  )
}

function Filter({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
      <span className="dim3" style={{ fontSize: 10 }}>{label}</span>
      {children}
    </span>
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
