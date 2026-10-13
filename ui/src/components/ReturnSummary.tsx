// 归来摘要 ReturnSummary（ui-redesign 票 05，components C12、interaction §7、workbench#return）。
// 钉在对话顶部的 raised 卡：离开区间 + 一句话结论 + KPI 四格 + 「需要你处理」+ 建议下一步；
// 可收起为一行 turnsummary 形。时间线里的同一事件只渲成一行标记，点开就地展开。
// 裁决不在这里发生：首行只标出顶卡键位（⌘↵ 由 usePendingKeys / handlePendingKey 处理，
// 与待决坞是同一张卡）；建议只填入输入条（fillComposer），绝不发送。
import { useEffect, useMemo, useState } from 'react'
import { Trans, useTranslation } from 'react-i18next'
import type { PendingQuestion, TimelineItem } from '../api'
import { api } from '../api'
import { useUiStore } from '../store'
import { bindingFor, matches } from '../keymap'
import { kindTitleKey } from '../decisions'
import i18n from '../i18n'
import { fmtYuan, parseTime } from '../usage'
import { fillComposer } from '../composerFill'
import { ATTENTION_KEYS, attentionRows, awayRange, readSummary, suggestions, type AttentionTone } from '../returnSummaryModel'
import { Icon, type IconName } from './Icon'
import { IconButton } from './ui/icon-button'
import { ShortcutKbd } from './ui/kbd'

const TONE: Record<AttentionTone, { icon: IconName; bg: string; fg: string }> = {
  irreversible: { icon: 'publish', bg: 'var(--red-tint)', fg: 'var(--red)' },
  authorize: { icon: 'shield', bg: 'var(--orange-tint)', fg: 'var(--orange)' },
  unknown: { icon: 'warn', bg: 'var(--canvas)', fg: 'var(--ink-2)' },
}

/** 卡上的一句事实：命令、远端、阶段或目标——只取载荷已有字段，不拼推测。 */
function pendingDetail(q: PendingQuestion): string {
  const p = q.payload as Record<string, unknown>
  const input = (p.input ?? {}) as Record<string, unknown>
  const v = input.cmd ?? input.path ?? p.remote ?? p.stage ?? p.target ?? p.instruction ?? p.tool
  return v == null ? '' : String(v)
}

/** 离开期间花费：usage 账本按 since_event 时刻起算（usage_series 的 from 是 created_at 下界）。 */
function useAwayCost(from: string | null): { loading: boolean; mc: number | null } {
  const [state, setState] = useState<{ key: string | null; mc: number | null }>({ key: null, mc: null })
  useEffect(() => {
    if (!from) return
    let live = true
    api.usageSeries('hour', from)
      .then((rows) => { if (live) setState({ key: from, mc: rows.reduce((sum, r) => sum + r.cost_mc, 0) }) })
      .catch(() => { if (live) setState({ key: from, mc: null }) })
    return () => { live = false }
  }, [from])
  return { loading: from != null && state.key !== from, mc: state.key === from ? state.mc : null }
}

/** 区间只写时分（workbench#return「13:40 – 14:02」）；跨日时 Intl 自带日期前缀没有意义，归来摘要只覆盖一次离开。 */
function hm(iso: string): string {
  const d = new Date(parseTime(iso))
  return Number.isNaN(d.getTime()) ? '' : new Intl.DateTimeFormat(i18n.language, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }).format(d)
}

function rangeText(t: (k: string, o?: Record<string, unknown>) => string, item: TimelineItem, items: readonly TimelineItem[]) {
  const r = awayRange(item, items)
  const to = hm(r.to)
  return {
    from: r.from,
    minutes: r.minutes,
    text: r.from && r.minutes != null
      ? t('returnSummary.range', { from: hm(r.from), to, minutes: r.minutes })
      : t('returnSummary.rangeUntil', { to }),
  }
}

export function ReturnSummaryCard({ item, items, onJumpEvent, pinned = true }: {
  item: TimelineItem
  items: readonly TimelineItem[]
  onJumpEvent?: (eventId: number) => void
  pinned?: boolean
}) {
  const { t } = useTranslation()
  const pending = useUiStore((s) => s.pending)
  const [collapsed, setCollapsed] = useState(false)
  const summary = useMemo(() => readSummary(item.event.payload as Record<string, unknown>), [item])
  const rows = useMemo(() => attentionRows(pending), [pending])
  const next = useMemo(() => suggestions(summary), [summary])
  const range = rangeText(t, item, items)
  const cost = useAwayCost(range.from)

  // returnSummaryToggle：默认无绑定，负责人可在设置·键盘里指定（只作用于钉顶那张）
  useEffect(() => {
    if (!pinned) return
    const h = (e: KeyboardEvent) => {
      if (useUiStore.getState().modalScope !== 'workbench') return
      if (matches(e, bindingFor('returnSummaryToggle'))) { e.preventDefault(); setCollapsed((c) => !c) }
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [pinned])

  if (collapsed) {
    return (
      <button type="button" className="sysrow turn-summary" data-return-summary="collapsed" style={{ width: '100%', margin: '12px 0 4px', border: 0, font: 'inherit' }}
        aria-expanded={false} onClick={() => setCollapsed(false)}>
        <Icon name="list" size={14} />
        <span className="tl-ts-l">{range.minutes != null
          ? t('returnSummary.collapsedLine', { minutes: range.minutes, count: rows.length })
          : t('returnSummary.collapsedLineNoTime', { count: rows.length })}</span>
        <span className="tl-sp" />
        <Icon name="chevron-right" size={12} />
      </button>
    )
  }

  const since = summary.since_event
  const attention = ATTENTION_KEYS.filter((k) => summary.attention[k] > 0)
  return (
    <section data-return-summary="" aria-label={t('cards.returnSummary')} className="tl-rs">
      <div className="tl-rs-h">
        <b>{t('cards.returnSummary')}</b>
        {since > 0 && onJumpEvent
          ? <button type="button" className="tl-rs-range" title={t('rs.jumpToMark')} onClick={() => onJumpEvent(since)}>{range.text}</button>
          : <span className="tl-rs-range">{range.text}</span>}
        <span className="tl-sp" />
        <IconButton size="sm" label={t('returnSummary.collapse')} action="returnSummaryToggle" onClick={() => setCollapsed(true)}>
          <Icon name="chevron-up" size={12} />
        </IconButton>
      </div>
      <p className="tl-rs-lead">
        <Trans
          i18nKey={rows.length ? 'returnSummary.headline' : 'returnSummary.headlineClear'}
          values={{ stages: summary.stages.finished, deliveries: summary.deliveries.length, pending: rows.length }}
          components={{ b: <b className="tl-rs-em" />, o: <b className="tl-rs-em" style={{ color: 'var(--orange)' }} /> }}
        />
      </p>
      <div className="tl-rs-kpi">
        <div className="tl-rs-cell"><span>{t('returnSummary.kpiStages')}</span><b>{t('returnSummary.kpiStagesValue', { count: summary.stages.finished })}</b></div>
        <div className="tl-rs-cell"><span>{t('returnSummary.kpiDeliveries')}</span><b>{t('returnSummary.kpiDeliveriesValue', { count: summary.deliveries.length })}</b></div>
        <div className="tl-rs-cell"><span>{t('returnSummary.kpiReviews')}</span><b>{t('returnSummary.kpiReviewsPassed', { count: summary.reviews.passed })}{summary.reviews.rejected > 0 && <> · <span style={{ color: 'var(--red)' }}>{t('returnSummary.kpiReviewsRejected', { count: summary.reviews.rejected })}</span></>}</b></div>
        <div className="tl-rs-cell"><span>{t('returnSummary.kpiCost')}</span>
          {cost.loading ? <i className="tl-rs-skl" aria-label={t('returnSummary.costLoading')} /> : <b title={cost.mc == null ? t('returnSummary.costUnknown') : undefined}>{cost.mc == null ? '—' : fmtYuan(cost.mc)}</b>}
        </div>
      </div>
      <div className="tl-rs-label">{t('returnSummary.needsYou')}</div>
      {rows.length === 0 && attention.length === 0
        ? <div className="tl-note">{t('returnSummary.nothingNeeded')}</div>
        : (
          <ul className="tl-rs-list">
            {rows.map(({ question, tone }, i) => {
              const style = TONE[tone]
              const detail = pendingDetail(question)
              return (
                <li key={question.id} data-attention={tone} style={{ background: style.bg }}>
                  <Icon name={style.icon} size={14} style={{ color: style.fg }} />
                  <span className="tl-grow"><b>{t(kindTitleKey(question))}</b>{detail && <> · <span className={tone === 'irreversible' ? undefined : 'mono'}>{detail}</span></>}</span>
                  {/* 首行 = 坞顶卡：显示当前 approve 绑定（ADR 0051，按平台渲染 ⌘ / Ctrl） */}
                  {i === 0 && <ShortcutKbd binding={bindingFor('approve')} />}
                </li>
              )
            })}
            {attention.map((key) => (
              <li key={key} data-attention="unknown" style={{ background: TONE.unknown.bg }}>
                <Icon name="warn" size={14} style={{ color: TONE.unknown.fg }} />
                <span className="tl-grow">{t(`returnAttention.${key}`, { count: summary.attention[key] })}</span>
              </li>
            ))}
          </ul>
        )}
      <div className="tl-rs-label">{t('returnSummary.next')}</div>
      <div className="tl-rs-next">
        {next.map((s) => {
          const text = t(`returnSummary.suggest.${s.key}`, s.params)
          return (
            <button key={s.key} type="button" data-suggestion={s.key} onClick={() => fillComposer(text)} title={t('returnSummary.fillHint')}>
              <Icon name="arrow-right" size={12} />
              <span>{text}</span>
            </button>
          )
        })}
      </div>
    </section>
  )
}

/** 时间线里的归来摘要事件：一行标记，点开就地展开完整卡（历史回看用）。 */
export function ReturnSummaryMarker({ item, onJumpEvent }: { item: TimelineItem; onJumpEvent?: (eventId: number) => void }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const range = rangeText(t, item, [])
  if (open) {
    return (
      <div>
        <ReturnSummaryCard item={item} items={[]} onJumpEvent={onJumpEvent} pinned={false} />
        <button type="button" className="sysrow tl-sys" aria-expanded onClick={() => setOpen(false)}>
          <span className="syslabel tl-sys-l">{t('returnSummary.collapse')} <Icon name="chevron-up" size={12} /></span>
        </button>
      </div>
    )
  }
  return (
    <button type="button" className="sysrow tl-sys" aria-expanded={false} data-return-marker="" onClick={() => setOpen(true)}>
      <span className="syslabel tl-sys-l"><Icon name="list" size={12} />{t('cards.returnSummary')} · {range.text} <Icon name="chevron-right" size={12} /></span>
    </button>
  )
}
