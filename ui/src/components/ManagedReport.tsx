import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type PendingQuestion } from '../api'
import { useUiStore } from '../store'
import type { ExternalOperation } from '../gen/ExternalOperation'
import type { ManagedRecord } from '../gen/ManagedRecord'
import { TIMELINE_JUMP_EVENT } from '../timelineJump'
import { useDelegation, useManagedHistory } from '../useDelegation'
import { kindTitleKey } from '../decisions'
import { parseTraceTime, sessionMinutes } from '../managedView'
import { exportManagedReport, managedReportMarkdown, spendText } from '../managedReportExport'
import { bindingFor, formatBinding, matches } from '../keymap'
import { toastError } from '../toast'
import type { ManagedSummary } from '../gen/ManagedSummary'
import { Empty } from '@/components/ui/empty'
import { Icon } from './Icon'
import { Badge } from './ui/badge'
import { Button } from './ui/button'
import { SkeletonRows } from './settings/kit'
import './ProjectDelegation.css'

/** How many completed items show before「另外 N 项」. */
const DONE_SHOWN = 3

const show = (eventId: number) => window.dispatchEvent(new CustomEvent(TIMELINE_JUMP_EVENT, { detail: eventId }))

/** Test3/23: what delegation decided and what really happened. Only a
 * landed effect is "done"; an empty pending list is never success.
 * ui-redesign 票 08（components D9 / managed#report）：头（bot + 托管报告 + 时段）→ KPI 四格 →
 * 「未完成」置前（原因 + 下一步）→「已完成」（依据 chip）+「另外 N 项」→ 外部操作（核实标记）。
 * `card`：独立卡形（胶囊弹出的报告对话框、归来置顶）；否则平铺（设置 · 权限的折叠内）。 */
export function ManagedReport({ card = false, onClose }: { card?: boolean; onClose?: () => void }) {
  const { t } = useTranslation()
  const { history, failed } = useManagedHistory()
  const { summary } = useDelegation()
  const [external, setExternal] = useState<ExternalOperation[]>([])
  const [more, setMore] = useState(false)
  const latest = useUiStore(s => s.timelineFacts?.latest_event_id ?? 0)
  const mounted = useRef(true)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  useEffect(() => {
    void api.managedExternalOps().then(ops => { if (mounted.current) setExternal(ops) }).catch(() => {})
  }, [latest])
  const records = history ?? []
  const unfinished = records.filter(r => r.result !== 'done')
  const done = records.filter(r => r.result === 'done')
  const shownDone = more ? done : done.slice(0, DONE_SHOWN)
  const period = periodText(t, summary)
  const exportReport = async () => {
    try {
      const markdown = managedReportMarkdown(t, { title: t('delegation.report'), period, records, external, summary,
        recordTitle: record => recordTitle(t, record) })
      if (await exportManagedReport(markdown)) useUiStore.getState().pushToast(t('delegation.exportDone'))
    } catch (error) { toastError(error) }
  }
  const exportRef = useRef(exportReport)
  useEffect(() => { exportRef.current = exportReport })
  useEffect(() => {
    if (!card) return
    const onKey = (event: KeyboardEvent) => {
      if (event.repeat || !matches(event, bindingFor('managedExport'))) return
      event.preventDefault(); void exportRef.current()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [card])
  const exportTip = [t('delegation.exportReport'), formatBinding(bindingFor('managedExport'))].filter(Boolean).join(' · ')
  const close = onClose
    ? (eventId: number) => { onClose(); show(eventId) }
    : show
  const body = failed
    ? <p role="alert" className="flex items-center gap-2 text-[12.5px] text-red"><Icon name="warn" size={14} />{t('delegation.readFailed')}</p>
    : history === null
      ? <SkeletonRows rows={3} label={t('delegation.report')} className="p-0" />
      : records.length === 0
        // 功能缺口 C3（A19）：托管期间没有可给的动作——决定由 Agent 在托管下做出；说明写明它们会记在这里。
        ? <Empty size="sm" icon="inbox" title={t('delegation.empty')} desc={t('delegation.emptyBody')} data-managed-empty />
        : <>
          <div className="grid grid-cols-4 gap-2.5" data-managed-kpi>
            <Kpi label={t('delegation.kpiDecisions')} value={records.length} />
            <Kpi label={t('delegation.kpiDone')} value={done.length} tone="text-green" />
            {/* gap A3（managed#report）：第四格由「失败」换成「花费」（本次托管时段）；失败并入「未完成」。 */}
            <Kpi label={t('delegation.kpiIncomplete')} value={unfinished.length} tone="text-red" />
            <Kpi label={t('delegation.kpiSpend')} value={summary ? spendText(t, summary) : '—'} />
          </div>
          <p className="mt-1.5 text-[12px] text-ink-3">{t('delegation.recent', { count: records.length })}</p>
          {unfinished.length > 0 && <>
            <h4 className="mt-4 mb-2 text-[12px] font-normal text-ink-3">{t('delegation.groupUnfinished')}</h4>
            <ul className="m-0 list-none overflow-hidden rounded-[12px] bg-[var(--app-card)] p-0 shadow-[var(--shadow-card)]" data-managed-unfinished>
              {unfinished.map(record => <li key={record.event_id} data-managed-record={record.event_id}
                className="delegation-unfinished flex min-h-[52px] items-center gap-3.5 px-4 py-2.5 [&+li]:border-t [&+li]:border-line-soft">
                <Icon name="circle-x" size={14} className="flex-none text-red" />
                <div className="min-w-0 flex-1">
                  <b className="block text-[13px] font-medium text-ink">{recordTitle(t, record)}</b>
                  <span className="block text-[12.5px] text-ink-2">{t(`delegation.next.${record.result}`)} · <code className="text-[11.5px]">{record.reason}</code></span>
                </div>
                <Button size="xs" title={t('delegation.showInTimeline')} onClick={() => close(record.event_id)}>{t('delegation.showInTimeline')}</Button>
              </li>)}
            </ul>
          </>}
          {done.length > 0 && <>
            <h4 className="mt-4 mb-2 text-[12px] font-normal text-ink-3">{t('delegation.groupDone')}</h4>
            <ul className="m-0 flex list-none flex-col p-0 text-[13px]" data-managed-done>
              {shownDone.map(record => <li key={record.event_id} data-managed-record={record.event_id}>
                <button type="button" className="hx-focus flex h-8 w-full items-center gap-2 rounded-[6px] px-1 text-left live:hover:bg-hover"
                  title={t('delegation.showInTimeline')} onClick={() => close(record.event_id)}>
                  <Icon name="circle-check" size={14} className="flex-none text-green" />
                  <span className="min-w-0 flex-1 truncate">{recordTitle(t, record)}</span>
                  {/* gap A5：依据用宿主的证据分类（真实检查 / 有质量证据 / 宿主已核实 / 项目内 / 模型选择 / 宿主规则）。 */}
                  <span className="text-[12px] text-ink-3" data-evidence={record.evidence}>{t(`delegation.evidence.${record.evidence}`)}</span>
                </button>
              </li>)}
            </ul>
            {done.length > DONE_SHOWN && <button type="button" className="hx-focus mt-1 flex items-center gap-1 rounded-[6px] text-[12px] text-ink-2 live:hover:text-ink"
              aria-expanded={more} onClick={() => setMore(!more)}>
              <Icon name={more ? 'chevron-up' : 'chevron-down'} size={12} />{t('delegation.more', { count: done.length - DONE_SHOWN })}
            </button>}
          </>}
        </>
  return <section data-managed-report className={card ? 'rounded-[14px] bg-surface px-[18px] py-4 shadow-[var(--shadow-raised)]' : undefined}
    aria-label={t('delegation.report')}>
    {card && <div className="mb-3 flex items-center gap-2">
      <span className="mg-av"><Icon name="bot" size={12} /></span>
      <b className="text-[15px] font-semibold text-ink">{t('delegation.report')}</b>
      {period && <span className="text-[13px] text-ink-3" data-managed-period>{period}</span>}
      <span className="flex-1" />
      {records.length > 0 && <Button size="xs" variant="ghost" onClick={() => { void exportReport() }} title={exportTip}>
        <Icon name="export" size={12} />{t('delegation.export')}
      </Button>}
      {onClose && <Button size="xs" variant="ghost" onClick={onClose} title={t('topbar.flowClose')} aria-label={t('topbar.flowClose')}>
        <Icon name="close" size={12} />
      </Button>}
    </div>}
    {body}
    <ExternalOperations ops={external} onShow={close} />
  </section>
}

/** gap A2：报告头时段「14:20 – 15:02 · 42 分钟」；仍在托管时止于现在。 */
function periodText(t: (key: string, opts?: Record<string, unknown>) => string, summary: ManagedSummary | null): string | null {
  const from = summary?.started_at ? parseTraceTime(summary.started_at) : null
  const minutes = summary ? sessionMinutes(summary.started_at, summary.ended_at) : null
  if (!summary || !from || minutes == null) return null
  const to = summary.ended_at ? parseTraceTime(summary.ended_at) ?? new Date() : new Date()
  const hm = (d: Date) => d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  return t('delegation.period', { from: hm(from), to: hm(to), count: minutes })
}

function Kpi({ label, value, tone }: { label: string; value: number | string; tone?: string }) {
  return <div className="rounded-[10px] bg-inset p-3">
    <div className="text-[12px] text-ink-3">{label}</div>
    <div className={`text-[15px] font-semibold tabular-nums ${tone ?? 'text-ink'}`}>{value}</div>
  </div>
}

function recordTitle(t: (key: string, opts?: Record<string, unknown>) => string, record: ManagedRecord): string {
  const kind = t(kindTitleKey({ id: record.card_id, kind: record.card_kind, agent_id: null, payload: {}, state: 'queued' } as unknown as PendingQuestion),
    { defaultValue: record.card_kind, who: '', tool: '', stage: '', name: '' })
  return `${t(`delegation.action.${record.action}`, { defaultValue: record.action })} · ${kind}`
}

const VERIFY_TONE: Record<string, 'ok' | 'neutral' | 'warn'> = {
  host_verified_recoverable: 'ok', host_verified_worktree_created: 'ok', host_verified_lossless_removal: 'ok',
  host_verified_not_moved: 'neutral', host_unverified_deletion: 'warn',
}

/** Ticket 05: what delegation did outside the project. Local items moved to
 * the Trash are recoverable; app, MCP and browser actions are not verified
 * by the host, and the report says so on every entry (D9 外部操作 · 三种核实标记). */
function ExternalOperations({ ops, onShow }: { ops: ExternalOperation[]; onShow: (eventId: number) => void }) {
  const { t } = useTranslation()
  return <section className="mt-4" data-external-ops>
    <h4 className="mb-2 text-[12px] font-normal text-ink-3">{t('delegation.externalTitle')}</h4>
    {ops.length === 0
      ? <p className="delegation-empty">{t('delegation.externalEmpty')}</p>
      : <ul className="m-0 flex list-none flex-col gap-1 p-0 text-[12.5px] text-ink-2">
        {ops.map(op => <li key={op.event_id} data-external-kind={op.kind} data-verification={op.verification}
          className="flex flex-wrap items-center gap-2 rounded-[8px] px-1 py-1 live:hover:bg-hover">
          <Badge shape="sq">{t(`delegation.externalKind.${op.kind}`, { defaultValue: op.kind })}</Badge>
          <code className="min-w-0 truncate font-mono text-[12px] text-ink">{op.target}</code>
          {/* MCP tool names are the service's own identifiers: shown as is. */}
          <span>{t(`delegation.externalAction.${op.action}`, { defaultValue: op.action })}</span>
          <span>{t(`delegation.action.${op.decision}`, { defaultValue: op.decision })}</span>
          <Badge variant={op.result === 'done' ? 'ok' : op.result === 'failed' ? 'err' : 'warn'}>{t(`delegation.result.${op.result}`, { defaultValue: op.result })}</Badge>
          <Badge variant={VERIFY_TONE[op.verification] ?? 'warn'} title={t(`delegation.verification.${op.verification}`, { defaultValue: op.verification })}>
            {t(`delegation.verifyChip.${op.verification}`, { defaultValue: op.verification })}
          </Badge>
          {op.trash_path && <span>{t('delegation.trashedTo', { path: op.trash_path })}</span>}
          <time className="text-[11.5px] text-ink-3" dateTime={op.at}>{parseTraceTime(op.at)?.toLocaleString() ?? op.at}</time>
          <Button size="xs" variant="ghost" title={t('delegation.showInTimeline')} onClick={() => onShow(op.event_id)}>
            {t('delegation.showInTimeline')}
          </Button>
        </li>)}
      </ul>}
  </section>
}
