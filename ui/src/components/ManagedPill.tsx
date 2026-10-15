import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { toastError } from '../toast'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches, type ActionId } from '../keymap'
import { useDelegation, useManagedHistory } from '../useDelegation'
import { Icon } from './Icon'
import { ManagedReport } from './ManagedReport'
import { Dot } from './ui/dot'
import { Popover, PopoverContent, PopoverTrigger } from './ui/popover'
import { Dialog, DialogContent, DialogTitle } from './ui/dialog'
import { Spinner } from './ui/spinner'
import './ProjectDelegation.css'

const tip = (label: string, action: ActionId) =>
  [label, formatBinding(bindingFor(action))].filter(Boolean).join(' · ')

type Busy = 'pause' | 'off' | null

/** 2026-10-11 UI alignment: the resident "delegating" status. Pause is a
 * safe-direction shortcut that works in any modal scope (like the
 * computer-use pause); turning on lives only in the approval-mode menu,
 * behind its explanation.
 * ui-redesign 票 08（components D7 / managed#exit）：胶囊改 accent 身份色（R-08 拍板；原「中性灰、
 * 不用琥珀」的意图保留——accent 是托管身份色，不是待决的橙）；暂停转灰。弹层 = 头 + kv + 报告 /
 * 暂停 / 关闭三行 + 脚注。报告改在 A17 Dialog（查看类）里打开；关闭成功后报告作为收场弹出，并 toast
 * 转回人工的件数。宿主确认后才变外观（行内环形加载，不乐观更新）。 */
export function ManagedPill() {
  const { t } = useTranslation()
  const { status, processing, apply } = useDelegation()
  const [open, setOpen] = useState(false)
  const [report, setReport] = useState(false)
  const [busy, setBusy] = useState<Busy>(null)
  const enabled = status?.enabled ?? false
  const paused = status?.paused ?? false
  const run = async (kind: Busy, write: (root: string) => Promise<unknown>) => {
    setBusy(kind)
    try {
      await apply(async root => { await write(root); return api.projectDelegation() })
      return true
    } catch (error) {
      toastError(error)
      return false
    } finally { setBusy(null) }
  }
  const pause = (next: boolean) => run('pause', root => api.setProjectDelegationPaused(next, root))
  const disable = async () => {
    const handed = processing
    if (!await run('off', root => api.setProjectDelegation(false, root))) return
    // interaction §8 关闭：未提交事项回到坞并 toast 件数；报告作为收场。
    useUiStore.getState().pushToast(t('delegation.offToast', { count: handed }))
    setOpen(false); setReport(true)
  }
  const current = useRef({ enabled, paused, pause, disable })
  useLayoutEffect(() => { current.current = { enabled, paused, pause, disable } })
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const { enabled, paused, pause, disable } = current.current
      if (!enabled || event.repeat) return
      if (!paused && matches(event, bindingFor('managedPause'))) { event.preventDefault(); void pause(true) }
      else if (paused && matches(event, bindingFor('managedResume'))) { event.preventDefault(); void pause(false) }
      else if (matches(event, bindingFor('managedDisable'))) { event.preventDefault(); void disable() }
      else if (matches(event, bindingFor('managedReport'))) { event.preventDefault(); setOpen(false); setReport(true) }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [])
  const label = paused ? t('delegation.pillPaused') : t('delegation.pillOn')
  return <>
    {enabled && <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button type="button" className={`mg-chip managed-pill hx-focus${paused ? ' paused' : ''}`}
          data-managed-state={paused ? 'paused' : 'on'} title={tip(t('delegation.report'), 'managedReport')}>
          {paused ? <Icon name="pause" size={12} /> : <Dot tone="info" pulse />}
          {label}
          <Icon name="chevron-down" size={12} />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[340px] p-1.5" aria-label={label}>
        <PillMenu paused={paused} processing={processing} busy={busy} label={label}
          onReport={() => { setOpen(false); setReport(true) }}
          onPause={() => { void pause(!paused) }} onOff={() => { void disable() }} />
      </PopoverContent>
    </Popover>}
    {/* 关闭托管后胶囊消失，报告对话框仍要留着作收场——所以挂在胶囊外。 */}
    <Dialog open={report} onOpenChange={setReport}>
      <DialogContent className="w-[min(720px,calc(100vw-48px))] max-h-[80vh] overflow-y-auto p-0" aria-describedby={undefined}>
        <DialogTitle className="sr-only">{t('delegation.report')}</DialogTitle>
        {report && <ManagedReport card onClose={() => setReport(false)} />}
      </DialogContent>
    </Dialog>
  </>
}

function PillMenu({ paused, processing, busy, label, onReport, onPause, onOff }: {
  paused: boolean; processing: number; busy: Busy; label: string
  onReport: () => void; onPause: () => void; onOff: () => void
}) {
  const { t } = useTranslation()
  const { history } = useManagedHistory()
  const [mode, setMode] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    void api.approvalMode().then(s => { if (live) setMode(s.mode) }).catch(() => {})
    return () => { live = false }
  }, [])
  const row = 'hx-press hx-focus flex h-8 w-full items-center gap-[9px] rounded-[7px] px-[9px] text-left text-[13px] outline-none live:hover:bg-hover focus-visible:bg-hover disabled:opacity-45 [&_svg]:shrink-0'
  return <div data-managed-menu>
    <div className="flex items-center gap-2 px-2.5 pt-2.5 pb-1.5">
      <span className="mg-av sm"><Icon name="bot" size={12} /></span>
      <b className="flex-1 text-[13px] font-semibold text-ink">{label}</b>
      {/* 「已 N 分钟」缺宿主的开启时刻（DelegationStatus 无 enabled_at），功能缺口，不猜。 */}
    </div>
    <div className="m-1 rounded-[10px] bg-inset px-3 py-2.5 text-[12px]">
      <div className="flex"><span className="flex-1 text-ink-3">{t('delegation.kvProcessing')}</span><span>{paused ? t('delegation.pausedNote') : t('delegation.items', { count: processing })}</span></div>
      {/* 预算 kv 缺宿主数据（功能缺口），不写假数。 */}
      <div className="mt-1 flex"><span className="flex-1 text-ink-3">{t('delegation.kvDecisions')}</span><span>{history ? t('delegation.times', { count: history.length }) : '—'}</span></div>
    </div>
    <button type="button" className={row} onClick={onReport} title={tip(t('delegation.report'), 'managedReport')}>
      <Icon name="list" size={16} className="text-ink-2" />{t('delegation.report')}
    </button>
    <button type="button" className={row} disabled={busy !== null} onClick={onPause}
      title={paused ? tip(t('delegation.resume'), 'managedResume') : tip(t('delegation.pause'), 'managedPause')}>
      {busy === 'pause' ? <Spinner size={16} /> : <Icon name={paused ? 'bot' : 'pause'} size={16} className="text-ink-2" />}
      {paused ? t('delegation.resume') : t('delegation.pause')}
      <span className="ml-auto pl-3 text-[12px] text-ink-3">{paused ? t('delegation.resumeDesc') : t('delegation.pauseDesc')}</span>
    </button>
    <div className="mx-1 my-1 h-px bg-line" />
    <button type="button" className={`${row} text-red`} disabled={busy !== null} onClick={onOff} title={tip(t('delegation.turnOff'), 'managedDisable')}>
      {busy === 'off' ? <Spinner size={16} /> : <Icon name="close" size={16} />}
      {t('delegation.turnOff')}
      {mode && <span className="ml-auto pl-3 text-[12px] text-ink-3">{t('delegation.turnOffDesc', { mode: t(`approvalMode.${mode}`) })}</span>}
    </button>
    <p className="px-2.5 pt-1 pb-2 text-[12px] text-ink-3">{t('delegation.exitNote')}</p>
  </div>
}
