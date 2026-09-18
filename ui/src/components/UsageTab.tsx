import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type UsagePoint } from '../api'
import { useUiStore } from '../store'
import { breakdownRows, capReached, centsToMc, fmtYuan as fmtY, groupCost, parseLimitInput, seriesMax } from '../usage'

export function UsageTab() {
  const { t } = useTranslation()
  const { usageRows, usageTotal, team, refresh } = useUiStore()
  const [series, setSeries] = useState<UsagePoint[]>([])
  const [editLimit, setEditLimit] = useState('')
  const [editing, setEditing] = useState(false)

  useEffect(() => {
    api.usageSeries().then(setSeries).catch(() => setSeries([]))
  }, [usageRows])

  const spent = usageTotal?.spent_mc ?? 0
  const limitMc = usageTotal?.limit_cents != null ? centsToMc(usageTotal.limit_cents) : null
  const capped = capReached(spent, usageTotal?.limit_cents)
  const pct = limitMc ? Math.min(100, (spent / limitMc) * 100) : 0

  const rows = breakdownRows(usageRows)
  const roleOf = (id?: string | null) => team.find((m) => m.id === id)?.role ?? id ?? '—'
  const byAgent = groupCost(rows, (r) => roleOf(r.agent_id))
  const byModel = groupCost(rows, (r) => r.model || '—')
  const byStage = groupCost(rows, (r) => r.stage || t('usage.noStage'))

  return (
    <div style={{ flex: 1, overflowY: 'auto', padding: '10px 12px', fontSize: 12, display: 'flex', flexDirection: 'column', gap: 12 }}>
      {capped && (
        <div className="chip err" style={{ padding: '6px 10px', fontSize: 12 }}>
          ⚠ {t('usage.capHit')}
        </div>
      )}

      {/* 总计 + 上限 */}
      <div>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
          <span className="dim">{t('topbar.usage')}</span>
          <span className="mono" style={{ fontWeight: 560 }}>
            {fmtY(spent)}{limitMc != null && ` / ${fmtY(limitMc)}`}
          </span>
        </div>
        <div style={{ height: 4, borderRadius: 2, background: 'var(--bg-3)', marginTop: 6, overflow: 'hidden' }}>
          <div style={{ width: `${pct}%`, height: '100%', background: capped ? 'var(--err)' : 'var(--accent)', transition: 'width .3s' }} />
        </div>
        {editing ? (
          <div style={{ display: 'flex', gap: 6, marginTop: 8 }}>
            <input
              value={editLimit}
              onChange={(e) => setEditLimit(e.target.value)}
              placeholder={t('usage.limitHint')}
              className="mono"
              style={{ flex: 1, fontSize: 11, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px', outline: 'none' }}
            />
            <button
              className="btn primary"
              style={{ fontSize: 11 }}
              onClick={async () => {
                await api.setUsageLimit(parseLimitInput(editLimit))
                setEditing(false)
                await refresh()
              }}
            >
              {t('cards.confirm')}
            </button>
            <button className="btn" style={{ fontSize: 11 }} onClick={() => setEditing(false)}>✕</button>
          </div>
        ) : (
          <button className="btn" style={{ fontSize: 11, marginTop: 8 }} onClick={() => { setEditLimit(usageTotal?.limit_cents ? String(usageTotal.limit_cents / 100) : ''); setEditing(true) }}>
            {t('usage.setLimit')}
          </button>
        )}
      </div>

      {/* 按 Agent / 模型 / 阶段分解 */}
      <Section title={t('usage.byAgent')}>
        {byAgent.map(([role, cost]) => (
          <BarRow key={role} label={role} value={cost} max={byAgent[0]?.[1] ?? 1} />
        ))}
      </Section>
      <Section title={t('usage.byModel')}>
        {byModel.map(([model, cost]) => (
          <BarRow key={model} label={model} value={cost} max={byModel[0]?.[1] ?? 1} />
        ))}
      </Section>
      <Section title={t('usage.byStage')}>
        {byStage.map(([stage, cost]) => (
          <BarRow key={stage} label={stage} value={cost} max={byStage[0]?.[1] ?? 1} />
        ))}
      </Section>

      {/* 日曲线 */}
      {series.length > 0 && (
        <Section title={t('usage.daily')}>
          <Sparkline points={series} />
        </Section>
      )}
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="dim3" style={{ fontSize: 10, fontWeight: 560, marginBottom: 6 }}>{title}</div>
      {children}
    </div>
  )
}

function BarRow({ label, value, max, detail }: { label: string; value: number; max: number; detail?: string }) {
  return (
    <div style={{ marginBottom: 6 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11 }}>
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{label}</span>
        <span className="mono dim">{fmtY(value)}</span>
      </div>
      <div style={{ height: 3, borderRadius: 2, background: 'var(--bg-3)', marginTop: 3 }}>
        <div style={{ width: `${(value / max) * 100}%`, height: '100%', background: 'var(--accent)', borderRadius: 2 }} />
      </div>
      {detail && <div className="dim3" style={{ fontSize: 10, marginTop: 1 }}>{detail}</div>}
    </div>
  )
}

/** 轻量 SVG 面积曲线（ECharts 延后决策——用量数据量级不需要图表库）。 */
function Sparkline({ points }: { points: UsagePoint[] }) {
  const W = 220
  const H = 56
  const max = seriesMax(points)
  const step = points.length > 1 ? W / (points.length - 1) : W
  const xy = points.map((p, i) => [i * step, H - 4 - (p.cost_mc / max) * (H - 12)] as const)
  const line = xy.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ')
  const area = `${line} L${W},${H} L0,${H} Z`
  return (
    <div>
      <svg width={W} height={H} style={{ display: 'block' }}>
        <path d={area} fill="var(--accent-soft)" />
        <path d={line} fill="none" stroke="var(--accent)" strokeWidth={1.5} />
        {xy.map(([x, y], i) => (
          <circle key={i} cx={x} cy={y} r={2} fill="var(--accent)" />
        ))}
      </svg>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 9 }} className="dim3">
        <span>{points[0]?.day.slice(5)}</span>
        <span className="mono">{fmtY(points[points.length - 1]?.cost_mc)}</span>
        <span>{points[points.length - 1]?.day.slice(5)}</span>
      </div>
    </div>
  )
}
