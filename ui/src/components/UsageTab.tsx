import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type UsageBucket } from '../api'
import { useUiStore } from '../store'
import { agentColor } from '../colors'
import { Icon } from './Icon'
import {
  capReached, centsToMc, fmtTok, fmtYuan as fmtY,
  groupTokens, parseLimitInput, perAgentSeries,
} from '../usage'

/** Charges without a reliable price or supplier usage remain visible. */
export function UsageUncertainty() {
  const { t } = useTranslation()
  const total = useUiStore((s) => s.usageTotal)
  if (!total || !(total.unknown_requests > 0 || total.legacy_unknown_records > 0 || total.unknown_token_records > 0)) return null
  return <div className="dim3" role="status" style={{ marginTop: 6 }}>
    <span>{t('usage.unknownCost')}</span>
    {total.unknown_token_records > 0 && <span> · {t('usage.unknownTokens')}</span>}
    {total.legacy_unknown_records > 0 && <span> · {t('usage.historicalUnknown', { count: total.legacy_unknown_records })}</span>}
    {total.limit_cents != null && <div>{t('usage.budgetUnknown')}</div>}
  </div>
}

export function UsageTab({ onDetail }: { onDetail?: () => void }) {
  const { t } = useTranslation()
  const { usageRows, usageTotal, contextPressure, team, invalidate, openTab } = useUiStore()
  const [series, setSeries] = useState<UsageBucket[]>([])
  const [editLimit, setEditLimit] = useState('')
  const [editing, setEditing] = useState(false)

  useEffect(() => {
    const from = new Date(Date.now() - 29 * 864e5).toISOString().slice(0, 10)
    api.usageSeries('day', from).then(setSeries).catch(() => setSeries([]))
  }, [usageRows])

  const spent = usageTotal?.spent_mc ?? 0
  const limitMc = usageTotal?.limit_cents != null ? centsToMc(usageTotal.limit_cents) : null
  const capped = capReached(spent, usageTotal?.limit_cents)
  const pct = limitMc ? Math.min(100, (spent / limitMc) * 100) : 0

  const rows = usageRows
  const roleOf = (id?: string | null) => team.find((m) => m.id === id)?.role ?? id ?? '—'
  const byAgent = groupTokens(rows, (r) => roleOf(r.agent_id))
  const byStage = groupTokens(rows, (r) => r.stage || t('usage.noStage'))
  const agentSeries = perAgentSeries(series)

  return (
    <div style={{ flex: 1, overflowY: 'auto', padding: '10px 12px', fontSize: 12, display: 'flex', flexDirection: 'column', gap: 12 }}>
      {capped && (
        <div className="chip err" style={{ padding: '6px 10px', fontSize: 12 }}>
          <Icon name="warn" size={11} /> {t('usage.capHit')}
        </div>
      )}

      {/* 已记录 token + 已知金额预算条；未知部分单列，不代表供应商账单上限 */}
      <div>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
          <span className="dim">{t('usage.totalTokens')}</span>
          <span className="mono" style={{ fontWeight: 560 }}>{fmtTok(usageTotal?.tokens ?? 0)}</span>
        </div>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginTop: 4 }}>
          <span className="dim3" style={{ fontSize: 10 }}>{t('usage.knownAmount')}</span>
          <span className="mono dim" style={{ fontSize: 11 }}>
            {fmtY(spent)}{limitMc != null && ` / ${fmtY(limitMc)}`}
          </span>
        </div>
        {(usageTotal?.reserved_mc ?? 0) > 0 && <div className="dim3" role="status">
          {t('usage.reservedEstimate')} · {fmtY(usageTotal!.reserved_mc)}
        </div>}
        <UsageUncertainty />
        <div style={{ height: 4, borderRadius: 2, background: 'var(--bg-3)', marginTop: 6, overflow: 'hidden' }}>
          <div style={{ width: `${pct}%`, height: '100%', background: capped ? 'var(--err)' : 'var(--accent)', transition: 'width .3s' }} />
        </div>
        <div style={{ display: 'flex', gap: 6, marginTop: 8 }}>
          {editing ? (
            <>
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
                  await invalidate('usage')
                }}
              >
                {t('cards.confirm')}
              </button>
              <button className="btn" style={{ fontSize: 11, display: 'inline-flex', alignItems: 'center' }} onClick={() => setEditing(false)}><Icon name="close" size={11} /></button>
            </>
          ) : (
            <>
              <button className="btn" style={{ fontSize: 11 }} onClick={() => { setEditLimit(usageTotal?.limit_cents ? String(usageTotal.limit_cents / 100) : ''); setEditing(true) }}>
                {t('usage.setLimit')}
              </button>
              <button
                className="btn"
                style={{ fontSize: 11, marginLeft: 'auto' }}
                // 设置页嵌入时（ui-audit-2 票 05）：详情按钮回工作台开明细 tab
                onClick={() => (onDetail ? onDetail() : openTab({ id: 'usage', kind: 'usage', title: t('usage.detail') }))}
              >
                {t('usage.detail')}
              </button>
            </>
          )}
        </div>
      </div>

      {/* 撞限压力（context-window 票 03 / ADR 0068）：恢复层触发闸的度量面。
          ≥2 张撞限卡/两周 → 值得建恢复设计票；不届时不引入静默改写通道。 */}
      <Section title={t('usage.ctxPressure')}>
        <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11 }}>
          <span className="dim">{t('usage.overflowCards')}</span>
          <span className="mono">{contextPressure?.overflow_cards_14d ?? 0}</span>
        </div>
        <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, marginTop: 4 }}>
          <span className="dim">{t('usage.compactions')}</span>
          <span className="mono">{contextPressure?.compactions_14d ?? 0}</span>
        </div>
        <div className="dim3" style={{ fontSize: 10, marginTop: 6, lineHeight: 1.5 }}>{t('usage.overflowHint')}</div>
      </Section>

      {/* 按 Agent / 阶段分解（token 口径） */}
      <Section title={t('usage.byAgent')}>
        {byAgent.map(([role, tok]) => (
          <BarRow key={role} label={role} value={tok} max={byAgent[0]?.[1] ?? 1} />
        ))}
      </Section>
      <Section title={t('usage.byStage')}>
        {byStage.map(([stage, tok]) => (
          <BarRow key={stage} label={stage} value={tok} max={byStage[0]?.[1] ?? 1} />
        ))}
      </Section>

      {/* 按角色日 token 折线 */}
      {agentSeries.series.length > 0 && (
        <Section title={t('usage.daily')}>
          <MultiLine labels={agentSeries.labels} series={agentSeries.series} roleOf={roleOf} />
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

function BarRow({ label, value, max }: { label: string; value: number; max: number }) {
  return (
    <div style={{ marginBottom: 6 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11 }}>
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{label}</span>
        <span className="mono dim">{fmtTok(value)}</span>
      </div>
      <div style={{ height: 3, borderRadius: 2, background: 'var(--bg-3)', marginTop: 3 }}>
        <div style={{ width: `${(value / max) * 100}%`, height: '100%', background: 'var(--accent)', borderRadius: 2 }} />
      </div>
    </div>
  )
}

/** 右栏轻量多折线：每 Agent 一条，颜色 = 头像确定色。（票 17：顶栏 sparkline 复用） */
export function MultiLine({ labels, series, roleOf }: {
  labels: string[]
  series: { agentId: string; points: number[] }[]
  roleOf: (id?: string | null) => string
}) {
  const W = 220
  const H = 56
  const max = Math.max(1, ...series.flatMap((s) => s.points))
  const step = labels.length > 1 ? W / (labels.length - 1) : W
  return (
    <div>
      <svg width={W} height={H} style={{ display: 'block' }}>
        {series.map((s) => {
          const line = s.points
            .map((v, i) => `${i === 0 ? 'M' : 'L'}${(i * step).toFixed(1)},${(H - 4 - (v / max) * (H - 12)).toFixed(1)}`)
            .join(' ')
          return <path key={s.agentId} d={line} fill="none" stroke={agentColor(s.agentId, 55)} strokeWidth={1.5} />
        })}
      </svg>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 4 }}>
        {series.map((s) => (
          <span key={s.agentId} style={{ display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 10 }} className="dim3">
            <span style={{ width: 8, height: 2, background: agentColor(s.agentId, 55), borderRadius: 1 }} />
            {roleOf(s.agentId)}
          </span>
        ))}
      </div>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, marginTop: 2 }} className="dim3">
        <span>{labels[0]?.slice(5)}</span>
        <span className="mono">{fmtTok(max)}</span>
        <span>{labels[labels.length - 1]?.slice(5)}</span>
      </div>
    </div>
  )
}
