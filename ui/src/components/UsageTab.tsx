// 用量卡 UsageCard（components D20，usage#quick；ui-redesign 票 12）：侧栏用量入口弹出的 Popover 内容，
// 设置「用量」分区也嵌同一张卡（ui-audit-2 票 05：一份实现），外加上限编辑。
// 平时版：头「本月用量」+「详情 ›」；金额 + 「/ 上限」+ tokens；进度条；迷你 Area（90 高，chart-1）；
// 两格 kv（14 天撞限 / 机械压缩，context-window 票 03 / ADR 0068 的恢复层度量面）+ 恢复层阈值说明。
// 触顶版：红头「已达用量上限」+ 金额 + 说明 +「查看详情」「提高上限」。
// 未知费用 / 在途预占始终单列（Reliability 12：不把未知伪装成免费，也不混进已知金额）。
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import { Icon } from './Icon'
import { AreaTrend } from './usage/AreaTrend'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Progress } from '@/components/ui/progress'
import { cn } from '@/lib/utils'
import {
  capReached, centsToMc, dayAxis, daysAgo, fmtTok, fmtYuan, parseLimitInput, stackRows,
} from '../usage'

/** Charges without a reliable price or supplier usage remain visible. */
export function UsageUncertainty() {
  const { t } = useTranslation()
  const total = useUiStore((s) => s.usageTotal)
  if (!total || !(total.unknown_requests > 0 || total.legacy_unknown_records > 0 || total.unknown_token_records > 0)) return null
  return <div className="mt-1.5 text-[12px] text-ink-3" role="status" data-usage-uncertainty>
    <span>{t('usage.unknownCost')}</span>
    {total.unknown_token_records > 0 && <span> · {t('usage.unknownTokens')}</span>}
    {total.legacy_unknown_records > 0 && <span> · {t('usage.historicalUnknown', { count: total.legacy_unknown_records })}</span>}
    {total.limit_cents != null && <div>{t('usage.budgetUnknown')}</div>}
  </div>
}

/** 上限编辑：¥ 输入，留空 = 不限；↵ 确认、Esc 取消。 */
export function LimitEditor({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation()
  const total = useUiStore((s) => s.usageTotal)
  const [value, setValue] = useState(total?.limit_cents ? String(total.limit_cents / 100) : '')
  const [saving, setSaving] = useState(false)
  const [failed, setFailed] = useState(false)
  const save = async () => {
    setSaving(true)
    setFailed(false)
    try {
      await api.setUsageLimit(parseLimitInput(value))
      await useUiStore.getState().invalidate('usage')
      onDone()
    } catch {
      setFailed(true)
    } finally {
      setSaving(false)
    }
  }
  return (
    <div data-limit-editor className="mt-2">
      <div className="flex items-center gap-1.5">
        <Input
          autoFocus
          value={value}
          inputMode="decimal"
          aria-label={t('usage.setLimit')}
          aria-invalid={failed || undefined}
          placeholder={t('usage.limitHint')}
          className="h-7 font-mono text-[12px]"
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') { e.preventDefault(); void save() }
            if (e.key === 'Escape') { e.stopPropagation(); onDone() }
          }}
        />
        <Button size="xs" variant="primary" loading={saving} onClick={() => void save()}>{t('cards.confirm')}</Button>
        <Button size="xs" variant="ghost" onClick={onDone}>{t('usageCard.cancel')}</Button>
      </div>
      {failed && <div role="alert" className="mt-1 text-[12px] text-red">{t('usageCard.limitFailed')}</div>}
    </div>
  )
}

export const openUsageSettings = () =>
  window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: 'usage' }))

/** D20 卡体。onDetail 进入用量详情；onRaise 默认打开设置「用量」分区改上限。 */
export function UsageCard({ onDetail, onRaise = openUsageSettings }: { onDetail: () => void; onRaise?: () => void }) {
  const { t } = useTranslation()
  const total = useUiStore((s) => s.usageTotal)
  const series7d = useUiStore((s) => s.usageSeries7d)
  const pressure = useUiStore((s) => s.contextPressure)
  const spent = total?.spent_mc ?? 0
  const limitMc = total?.limit_cents != null ? centsToMc(total.limit_cents) : null
  const capped = capReached(spent, total?.limit_cents)
  const pct = limitMc ? Math.min(100, (spent / limitMc) * 100) : 0
  // 迷你趋势：store 已按 UTC 日拉了近 7 日（refreshSlow），按日补齐后合成一条
  const mini = useMemo(
    () => stackRows(series7d, dayAxis(daysAgo(6), daysAgo(0))).map((r) => ({ bucket: r.bucket, tokens: r.input + r.output + r.tool })),
    [series7d],
  )
  const miniTotal = mini.reduce((s, r) => s + r.tokens, 0)

  const amount = (
    <div className="mt-3 flex items-baseline gap-1.5">
      <span data-usage-amount className={cn('text-[22px] font-semibold tracking-[-0.02em] tabular-nums', capped ? 'text-red' : 'text-ink')}>
        {total ? fmtYuan(spent) : '—'}
      </span>
      <span className="text-[13px] text-ink-3">{limitMc != null ? t('usageCard.ofLimit', { limit: fmtYuan(limitMc) }) : t('usageCard.noLimit')}</span>
      <span className="flex-1" />
      {!capped && <span className="font-mono text-[12px] text-ink-3">{t('usageCard.tokens', { n: fmtTok(total?.tokens ?? 0) })}</span>}
    </div>
  )
  const extras = (
    <>
      {(total?.reserved_mc ?? 0) > 0 && <div className="mt-1.5 text-[12px] text-ink-3" role="status">
        {t('usage.reservedEstimate')} · {fmtYuan(total!.reserved_mc)}
      </div>}
      <UsageUncertainty />
    </>
  )

  if (capped) {
    return (
      <div data-usage-card="cap">
        <div className="flex items-center gap-2 rounded-[8px] bg-red-tint px-2.5 py-2 text-[12.5px] text-red">
          <Icon name="warn" size={14} />
          <b className="flex-1 font-semibold">{t('usageEntry.capLabel')}</b>
        </div>
        {amount}
        <Progress className="mt-2" value={100} tone="cap" aria-label={t('usageEntry.capLabel')} />
        {extras}
        <p className="mt-3 text-[12px] text-ink-2">{t('usageCard.capBody')}</p>
        <div className="mt-3 flex gap-2">
          <Button size="sm" className="flex-1" onClick={onDetail}>{t('usageCard.viewDetail')}</Button>
          <Button size="sm" variant="primary" className="flex-1" onClick={onRaise}>{t('usageEntry.raise')}</Button>
        </div>
      </div>
    )
  }

  return (
    <div data-usage-card="ok">
      <div className="flex items-center gap-2">
        <b className="flex-1 text-[13px] font-semibold text-ink">{t('usageCard.title')}</b>
        <Button size="xs" variant="ghost" data-usage-detail onClick={onDetail}>
          {t('usage.detail')}<Icon name="chevron-right" size={12} />
        </Button>
      </div>
      {amount}
      {limitMc != null && <Progress className="mt-2" value={pct} tone="accent" aria-label={t('usageEntry.label', { pct: Math.floor(pct) })} />}
      {extras}
      <div className="pt-3">
        <AreaTrend
          mini
          stacked={false}
          height={90}
          data={mini}
          series={[{ key: 'tokens', label: 'Token', color: 'var(--chart-1)' }]}
          ariaLabel={t('usageCard.miniAria', { total: fmtTok(miniTotal) })}
        />
      </div>
      <div className="mt-1 h-px bg-line" />
      <div className="mt-3 flex items-center text-[12px]">
        <span className="flex-1 text-ink-3">{t('usage.overflowCards')}</span>
        <span className="font-mono text-ink">{t('usageCard.times', { count: pressure?.overflow_cards_14d ?? 0 })}</span>
      </div>
      <div className="mt-1 flex items-center text-[12px]">
        <span className="flex-1 text-ink-3">{t('usage.compactions')}</span>
        <span className="font-mono text-ink">{t('usageCard.times', { count: pressure?.compactions_14d ?? 0 })}</span>
      </div>
      <div className="mt-2 text-[12px] text-ink-3">{t('usage.overflowHint')}</div>
    </div>
  )
}

/** 设置「用量」分区：同一张卡 + 上限编辑（设置上下文里「提高上限」就地编辑，不再跳转）。 */
export function UsageTab({ onDetail }: { onDetail?: () => void }) {
  const { t } = useTranslation()
  const [editing, setEditing] = useState(false)
  const detail = onDetail ?? (() => useUiStore.getState().openUsageDetail())
  return (
    <div className="flex-1 overflow-y-auto p-4">
      <Card className="max-w-[420px] px-4 py-3.5" data-usage-settings>
        <UsageCard onDetail={detail} onRaise={() => setEditing(true)} />
        {editing
          ? <LimitEditor onDone={() => setEditing(false)} />
          : <Button size="xs" variant="ghost" className="mt-2 self-start" onClick={() => setEditing(true)}>{t('usage.setLimit')}</Button>}
      </Card>
    </div>
  )
}
