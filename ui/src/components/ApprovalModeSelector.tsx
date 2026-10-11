import { blockBackgroundDecision } from '../modalDecisionGuard'
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { ApprovalMode } from '../gen/ApprovalMode'
import { bindingFor, formatBinding, matches } from '../keymap'
import { Icon } from './Icon'
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
  useEffect(() => {
    if (!open && !pending && restoreFocus.current) {
      restoreFocus.current = false
      trigger.current?.focus()
    }
  }, [open, pending])
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
      <Icon name={value === 'broad' ? 'warn' : 'shield'} />
      <span>{t(`approvalMode.${value}`)}{managed?.enabled && ` | ${t('delegation.short')}`}</span>
      <Icon name="chevron-down" />
    </button>
    {confirmBroad && <BroadConfirmation pending={pending} error={error}
      onCancel={() => { setConfirmBroad(false); setError(false); trigger.current?.focus() }}
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
      {modes.map((mode) => <button key={mode} type="button" role="menuitemradio"
        className="approval-mode-option" aria-checked={value === mode}
        disabled={pending || disabled} onClick={() => { void choose(mode) }}>
        <span className="approval-mode-option-copy"><strong>{t(`approvalMode.${mode}`)}</strong>
          <span>{t(`approvalMode.description.${mode}`)}</span></span>
        {value === mode && <Icon name="check" />}
      </button>)}
      <p className="approval-mode-boundary">{t('approvalMode.boundary')}</p>
      {managed && <div className="approval-mode-managed" role="group" aria-label={t('delegation.label')}>
        <label className="approval-mode-managed-row">
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
      </div>}
      {error && <p role="alert" className="approval-mode-error">{t('approvalMode.failed')}</p>}
    </div>}
  </div>
}


function BroadConfirmation({ pending, error, onCancel, onConfirm }: {
  pending: boolean; error: boolean; onCancel: () => void; onConfirm: () => void
}) {
  const { t } = useTranslation()
  const id = useId()
  const dialog = useRef<HTMLDialogElement>(null)
  const cancel = useRef<HTMLButtonElement>(null)
  const current = useRef({ pending, onCancel, onConfirm })
  useLayoutEffect(() => { current.current = { pending, onCancel, onConfirm } }, [pending, onCancel, onConfirm])
  useEffect(() => {
    if (dialog.current?.showModal) dialog.current.showModal()
    else dialog.current?.setAttribute('open', '')
    cancel.current?.focus()
    // Issue 10: native modality makes the background inert but does not stop
    // window decision listeners; a shortcut must never grant either permission.
    const block = (event: KeyboardEvent) => {
      const action = matches(event, bindingFor('cancelBroadAccess')) ? current.current.onCancel
        : matches(event, bindingFor('confirmBroadAccess')) ? current.current.onConfirm : null
      if (action) {
        event.preventDefault(); event.stopImmediatePropagation()
        if (!current.current.pending) action()
        return
      }
      if (!blockBackgroundDecision(event)) return
    }
    window.addEventListener('keydown', block, true)
    return () => window.removeEventListener('keydown', block, true)
  }, [])
  return <dialog ref={dialog} className="approval-mode-confirm" role="alertdialog" aria-modal="true"
    aria-labelledby={`${id}-title`} aria-describedby={`${id}-body`} aria-busy={pending}
    onCancel={event => { event.preventDefault(); if (!pending) onCancel() }}>
    <h2 id={`${id}-title`}><Icon name="warn" />{t('approvalMode.confirmTitle')}</h2>
    <div id={`${id}-body`}><p>{t('approvalMode.confirmBody')}</p><p>{t('approvalMode.boundary')}</p></div>
    {error && <p role="alert" className="approval-mode-error">{t('approvalMode.failed')}</p>}
    <div className="approval-mode-confirm-actions">
      <button ref={cancel} type="button" className="btn" disabled={pending} onClick={onCancel} title={[t('agent.cancel'), formatBinding(bindingFor('cancelBroadAccess')) || 'Esc'].join(' · ')}>{t('agent.cancel')}</button>
      <button type="button" className="btn approval-mode-broad" disabled={pending} onClick={onConfirm} title={[t('approvalMode.confirmAction'), formatBinding(bindingFor('confirmBroadAccess'))].filter(Boolean).join(' · ')}>{t('approvalMode.confirmAction')}</button>
    </div>
  </dialog>
}
