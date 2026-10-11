import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useDelegation } from '../useDelegation'
import { Icon } from './Icon'
import { ManagedReport } from './ManagedReport'
import './ProjectDelegation.css'

const tip = (label: string, action: Parameters<typeof bindingFor>[0]) =>
  [label, formatBinding(bindingFor(action))].filter(Boolean).join(' · ')

/** 2026-10-11 UI alignment: the resident "delegating" status. Neutral, never
 * the amber pending pulse — amber means "waiting for you", delegation means
 * "not yours to handle". Pause is a safe-direction shortcut that works in any
 * modal scope (like the computer-use pause); turning on lives only in the
 * approval-mode menu, behind its explanation. */
export function ManagedPill() {
  const { t } = useTranslation()
  const { status, processing, apply } = useDelegation()
  const [open, setOpen] = useState(false)
  const [showReport, setShowReport] = useState(false)
  const container = useRef<HTMLDivElement>(null)
  const enabled = status?.enabled ?? false
  const paused = status?.paused ?? false
  const run = (write: (root: string) => Promise<unknown>) =>
    apply(async root => { await write(root); return api.projectDelegation() })
      .catch(error => useUiStore.getState().pushToast(errText(error), 'err'))
  const pause = (next: boolean) => run(root => api.setProjectDelegationPaused(next, root))
  const disable = () => run(root => api.setProjectDelegation(false, root))
  const current = useRef({ enabled, paused, pause, disable })
  useLayoutEffect(() => { current.current = { enabled, paused, pause, disable } })
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const { enabled, paused, pause, disable } = current.current
      if (!enabled || event.repeat) return
      if (!paused && matches(event, bindingFor('managedPause'))) { event.preventDefault(); void pause(true) }
      else if (paused && matches(event, bindingFor('managedResume'))) { event.preventDefault(); void pause(false) }
      else if (matches(event, bindingFor('managedDisable'))) { event.preventDefault(); void disable() }
      else if (matches(event, bindingFor('managedReport'))) { event.preventDefault(); setOpen(true); setShowReport(true) }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [])
  useEffect(() => {
    if (!open) return
    const close = (event: PointerEvent) => {
      if (event.target instanceof Node && !container.current?.contains(event.target)) setOpen(false)
    }
    document.addEventListener('pointerdown', close)
    return () => document.removeEventListener('pointerdown', close)
  }, [open])
  if (!enabled) return null
  const label = paused ? t('delegation.pillPaused') : t('delegation.pillOn')
  return <div ref={container} className="managed-pill-wrap">
    <button type="button" className={`chip chip-btn managed-pill${paused ? ' managed-pill-paused' : ''}`}
      data-managed-state={paused ? 'paused' : 'on'} aria-expanded={open} onClick={() => setOpen(!open)}
      title={tip(t('delegation.report'), 'managedReport')}>
      <Icon name="agent" size={11} /> {label}
    </button>
    {open && <div className="managed-pill-popover" role="dialog" aria-label={label}>
      <p className="delegation-note">{paused ? t('delegation.pausedNote') : t('delegation.processing', { count: processing })}</p>
      <p className="delegation-note">{t('delegation.limits')}</p>
      <div className="delegation-confirm-actions">
        <button type="button" className="btn" onClick={() => setShowReport(!showReport)} title={tip(t('delegation.report'), 'managedReport')}>{t('delegation.report')}</button>
        {paused
          ? <button type="button" className="btn" onClick={() => { void pause(false) }} title={tip(t('delegation.resume'), 'managedResume')}>{t('delegation.resume')}</button>
          : <button type="button" className="btn" onClick={() => { void pause(true) }} title={tip(t('delegation.pause'), 'managedPause')}>{t('delegation.pause')}</button>}
        <button type="button" className="btn" onClick={() => { void disable() }} title={tip(t('delegation.turnOff'), 'managedDisable')}>{t('delegation.turnOff')}</button>
      </div>
      {showReport && <ManagedReport />}
    </div>}
  </div>
}
