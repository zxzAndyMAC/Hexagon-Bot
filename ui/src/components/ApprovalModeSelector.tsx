import { blockBackgroundDecision } from '../modalDecisionGuard'
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { ApprovalMode } from '../gen/ApprovalMode'
import { bindingFor, formatBinding, matches } from '../keymap'
import { Icon } from './Icon'
import { Badge } from '@/components/ui/badge'
import { Spinner } from '@/components/ui/spinner'
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogError, AlertDialogFooter, AlertDialogIcon, AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { ManagedConfirmation } from './ManagedConfirmation'
import './ProjectDelegation.css'
import './ApprovalModeSelector.css'

const modes: ApprovalMode[] = ['restricted', 'assisted', 'broad']

/** The parent supplies the persisted project value. Never optimistically claim
 * broader access before the host has accepted and persisted the change. */
/** 2026-10-11 UI alignment: delegation is a separate switch in this menu,
 * never a fourth mode — the three modes and the one in force stay as they
 * are, so turning delegation off returns to exactly that mode. */
export type ManagedControl = {
  enabled: boolean
  onEnable: () => Promise<void>
  onDisable: () => Promise<void>
}

export function ApprovalModeSelector({ value, onChange, disabled = false, shortcut = '', placement = 'above', managed }: {
  value: ApprovalMode
  onChange: (mode: ApprovalMode) => Promise<void>
  disabled?: boolean
  shortcut?: string
  placement?: 'above' | 'below'
  managed?: ManagedControl
}) {
  const { t } = useTranslation()
  const id = useId()
  const container = useRef<HTMLDivElement>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const [open, setOpen] = useState(false)
  const [confirmBroad, setConfirmBroad] = useState(false)
  const [confirmManaged, setConfirmManaged] = useState(false)
  const [managedPending, setManagedPending] = useState(false)
  const [managedError, setManagedError] = useState(false)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState(false)
  const inFlight = useRef(false)
  const restoreFocus = useRef(false)
  // 票 07：确认层是 Radix AlertDialog，开着时焦点困在里面——回到胶囊要等它卸载后（这里的 effect）再做。
  useEffect(() => {
    if (!open && !pending && !confirmBroad && !confirmManaged && restoreFocus.current) {
      restoreFocus.current = false
      trigger.current?.focus()
    }
  }, [open, pending, confirmBroad, confirmManaged])
  useEffect(() => {
    if (!open) return
    const close = (event: PointerEvent) => {
      if (event.target instanceof Node && !container.current?.contains(event.target)) setOpen(false)
    }
    document.addEventListener('pointerdown', close)
    container.current?.querySelector<HTMLButtonElement>('[aria-checked="true"]')?.focus()
    return () => document.removeEventListener('pointerdown', close)
  }, [open])
  async function choose(mode: ApprovalMode, confirmed = false) {
    if (inFlight.current || disabled) return
    if (mode === value) { setOpen(false); trigger.current?.focus(); return }
    // Owner decision 2026-10-02 / extension issue 10: each transition into
    // Broad needs explicit consent; opening an already Broad project does not.
    if (mode === 'broad' && !confirmed) { setOpen(false); setConfirmBroad(true); return }
    inFlight.current = true
    setPending(true)
    setError(false)
    try {
      await onChange(mode)
      restoreFocus.current = true
      setOpen(false)
      setConfirmBroad(false)
    } catch {
      setError(true)
    } finally {
      inFlight.current = false
      setPending(false)
    }
  }
  async function switchManaged(enable: boolean) {
    if (!managed || managedPending) return
    setManagedPending(true); setManagedError(false)
    try {
      // The host persists and re-verifies the grant first; only its answer
      // (through the parent) changes what this menu shows.
      await (enable ? managed.onEnable() : managed.onDisable())
      setConfirmManaged(false)
    } catch {
      setManagedError(true)
    } finally {
      setManagedPending(false)
    }
  }
  return <div ref={container} className="approval-mode" onKeyDown={(event) => {
    if (event.key === 'Escape' && open) {
      event.preventDefault()
      event.stopPropagation()
      setOpen(false)
      trigger.current?.focus()
    }
  }}>
    <button ref={trigger} type="button" className={`approval-mode-trigger${value === 'broad' ? ' approval-mode-broad' : ''}`}
      disabled={disabled || pending} aria-haspopup="menu" aria-expanded={open} aria-controls={id}
      title={[t('approvalMode.label'), shortcut].filter(Boolean).join(' · ')}
      aria-label={`${t('approvalMode.label')}: ${t(`approvalMode.${value}`)}`}
      onClick={() => { setError(false); setOpen(!open) }}>
      {/* components D4：shield 14 + 模式名 + ▾/▴；托管中「帮我批准 | 托管」，托管段 --accent-ink */}
      <Icon name="shield" size={14} />
      <span>{t(`approvalMode.${value}`)}{managed?.enabled && <><span className="sep"> | </span><span className="managed">{t('delegation.short')}</span></>}</span>
      <Icon name={open ? 'chevron-up' : 'chevron-down'} size={12} />
    </button>
    {confirmBroad && <BroadConfirmation pending={pending} error={error}
      onCancel={() => { restoreFocus.current = true; setConfirmBroad(false); setError(false) }}
      onConfirm={() => { void choose('broad', true) }} />}
    {confirmManaged && <ManagedConfirmation pending={managedPending} error={managedError}
      onCancel={() => { setConfirmManaged(false); setManagedError(false); trigger.current?.focus() }}
      onConfirm={() => { void switchManaged(true) }} />}
    {open && <div id={id} className={`approval-mode-menu approval-mode-menu-${placement}`} role="menu"
      aria-label={t('approvalMode.label')} aria-busy={pending}
      onKeyDown={(event) => {
        const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]'))
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
        const next = event.key === 'ArrowDown' ? (index + 1) % buttons.length
          : event.key === 'ArrowUp' ? (index - 1 + buttons.length) % buttons.length
          : event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : -1
        if (next >= 0) { event.preventDefault(); buttons[next]?.focus() }
        if (event.key === 'Tab') setOpen(false)
      }}>
      <div className="approval-mode-sec">{t('approvalMode.menuTitle')}</div>
      {modes.map((mode) => <button key={mode} type="button" role="menuitemradio"
        className="approval-mode-option" aria-checked={value === mode}
        disabled={pending || disabled} onClick={() => { void choose(mode) }}>
        {/* D4 加载：宿主复验前保持原选择，行内 ring（不乐观更新） */}
        <span className="tick">{value === mode && (pending ? <Spinner size={12} /> : <Icon name="check" size={14} />)}</span>
        <span className="approval-mode-option-copy"><strong>{t(`approvalMode.${mode}`)}</strong>
          <span>{t(`approvalMode.description.${mode}`)}</span></span>
        {mode === 'broad' && value !== 'broad' && <Badge variant="warn" className="approval-mode-chip">{t('approvalMode.needsConfirm')}</Badge>}
      </button>)}
      {managed && <>
      <div className="approval-mode-sepline" />
      <div className="approval-mode-managed" role="group" aria-label={t('delegation.label')}>
        <label className="approval-mode-managed-row">
          <Icon name="bot" size={14} />
          <span className="approval-mode-option-copy"><strong>{t('delegation.label')}</strong>
            <span>{t('delegation.description')}</span></span>
          <input type="checkbox" role="switch" className="switch" checked={managed.enabled}
            disabled={managedPending || disabled}
            title={[t(managed.enabled ? 'delegation.turnOff' : 'delegation.turnOn'),
              formatBinding(bindingFor(managed.enabled ? 'managedDisable' : 'managedEnable'))].filter(Boolean).join(' · ')}
            onChange={() => {
              // Turning on always passes the explanation; off is the safe direction.
              if (managed.enabled) void switchManaged(false)
              else { setOpen(false); setConfirmManaged(true) }
            }} />
        </label>
        {managedError && !confirmManaged && <p role="alert" className="approval-mode-error">{t('delegation.failed')}</p>}
      </div></>}
      {error && <p role="alert" className="approval-mode-error">{t('approvalMode.failed')}</p>}
    </div>}
  </div>
}


/** composer#broad（components A17 AlertDialog 变体）：首焦「取消」，确认默认无快捷键，取消保留 Esc。
 * 票 07 起由原生 dialog 换成共享 AlertDialog 外壳（Radix 负责焦点困住与首焦 Cancel）；
 * 背景裁决快捷键由 AlertDialogContent 的 modalDecisionGuard 拦截，重映射的确认 / 取消键仍在这里吃掉。 */
function BroadConfirmation({ pending, error, onCancel, onConfirm }: {
  pending: boolean; error: boolean; onCancel: () => void; onConfirm: () => void
}) {
  const { t } = useTranslation()
  const current = useRef({ pending, onCancel, onConfirm })
  useLayoutEffect(() => { current.current = { pending, onCancel, onConfirm } }, [pending, onCancel, onConfirm])
  useEffect(() => {
    // Issue 10: a shortcut must never grant either permission in the background.
    const block = (event: KeyboardEvent) => {
      const action = matches(event, bindingFor('cancelBroadAccess')) ? current.current.onCancel
        : matches(event, bindingFor('confirmBroadAccess')) ? current.current.onConfirm : null
      if (action) {
        event.preventDefault(); event.stopImmediatePropagation()
        if (!current.current.pending) action()
        return
      }
      blockBackgroundDecision(event)
    }
    window.addEventListener('keydown', block, true)
    return () => window.removeEventListener('keydown', block, true)
  }, [])
  const confirmKey = formatBinding(bindingFor('confirmBroadAccess'))
  return <AlertDialog open onOpenChange={(open) => { if (!open && !current.current.pending) current.current.onCancel() }}>
    <AlertDialogContent busy={pending} className="approval-mode-confirm w-[min(440px,calc(100vw-48px))] gap-0 px-[22px] pt-[22px] pb-4"
      onCloseAutoFocus={(event) => event.preventDefault()}>
      <div className="flex items-start gap-3">
        <AlertDialogIcon tone="warn" className="size-9 rounded-[10px]"><Icon name="shield" size={14} /></AlertDialogIcon>
        <div>
          <AlertDialogTitle>{t('approvalMode.confirmTitle')}</AlertDialogTitle>
          <AlertDialogDescription className="mt-1">{t('approvalMode.confirmBody')}</AlertDialogDescription>
        </div>
      </div>
      <ul className="broad-list mt-3">
        <li><Icon name="check" size={12} />{t('approvalMode.boundary')}</li>
      </ul>
      {error && <AlertDialogError className="mt-3">{t('approvalMode.failed')}</AlertDialogError>}
      <AlertDialogFooter className="mt-5">
        <AlertDialogCancel size="sm" onClick={(event) => { event.preventDefault(); onCancel() }}
          title={[t('agent.cancel'), formatBinding(bindingFor('cancelBroadAccess')) || 'Esc'].join(' · ')}>{t('agent.cancel')}</AlertDialogCancel>
        <AlertDialogAction size="sm" loading={pending} onClick={onConfirm}
          title={[t('approvalMode.confirmAction'), confirmKey].filter(Boolean).join(' · ')}>{t('approvalMode.confirmAction')}</AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
}
