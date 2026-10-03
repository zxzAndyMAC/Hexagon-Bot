import { blockBackgroundDecision } from '../modalDecisionGuard'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import type { DesktopPermissions as Status } from '../gen/DesktopPermissions'
import type { DesktopPermission } from '../gen/DesktopPermission'
import { bindingFor, formatBinding, matches } from '../keymap'


export function DesktopPermissions({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [status, setStatus] = useState<Status | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const running = useRef(false)
  const dialog = useRef<HTMLDialogElement>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const check = useCallback(async () => {
    if (running.current) return
    running.current = true; setBusy(true); setError('')
    try { setStatus(await api.desktopPermissions()) }
    catch (e) { setStatus(null); setError(errText(e)) }
    finally { running.current = false; setBusy(false) }
  }, [])
  const show = useCallback(() => { setOpen(true); void check() }, [check])
  useEffect(() => { window.addEventListener('hexagon:desktop-permissions', show); return () => window.removeEventListener('hexagon:desktop-permissions', show) }, [show])
  const close = () => { setOpen(false); trigger.current?.focus() }
  const settings = useCallback(async (permission: DesktopPermission) => {
    try { await api.desktopOpenSettings(permission); setError('') }
    catch (e) { setError(`${t('desktop.settingsFailed')} ${errText(e)}`) }
    // Opening a pane never updates grants. Only the next native check can do so.
  }, [t])
  useEffect(() => {
    if (!open) return
    if (dialog.current?.showModal) dialog.current.showModal()
    else dialog.current?.setAttribute('open', '')
    const focus = () => { void check() }
    window.addEventListener('focus', focus)
    return () => window.removeEventListener('focus', focus)
  }, [open, check])
  useEffect(() => {
    if (!open) return
    // Ticket 07 review: showModal makes the background inert, but window's
    // decision listener still receives keyboard events. Capture only decision
    // shortcuts; Escape and system-settings keys retain their normal behavior.
    const blockDecision = (event: KeyboardEvent) => {
      if (!blockBackgroundDecision(event)) return
      useUiStore.getState().pushToast(t('decisions.scopeBlocked'))
    }
    window.addEventListener('keydown', blockDecision, true)
    return () => window.removeEventListener('keydown', blockDecision, true)
  }, [open, t])
  useEffect(() => {
    const keys = (e: KeyboardEvent) => {
      if (matches(e, bindingFor('desktopPermissions'))) { e.preventDefault(); show() }
      if (!open || !status?.available) return
      if (matches(e, bindingFor('desktopAccessibility')) && (!status.accessibility || !status.input_events)) { e.preventDefault(); void settings('accessibility') }
      if (matches(e, bindingFor('desktopScreenRecording')) && !status.screen_recording) { e.preventDefault(); void settings('screen_recording') }
    }
    window.addEventListener('keydown', keys)
    return () => window.removeEventListener('keydown', keys)
  }, [open, status, settings, show])
  const tip = (key: 'desktopPermissions' | 'desktopAccessibility' | 'desktopScreenRecording', label: string) => `${label} · ${formatBinding(bindingFor(key))}`
  // Owner 2026-10-03: compact is a modal listener, not a settings row.
  // Hiding only its children left 33px of section spacing/border above TopBar.
  return <>
    {!compact && <section className="desktop-permissions">
    <div><strong>{t('desktop.title')}</strong><p className="dim3">{t('desktop.summary')}</p></div>
    <button ref={trigger} className="btn" onClick={show} title={tip('desktopPermissions', t('desktop.check'))}>{t('desktop.check')}</button>
    </section>}
    {open && <dialog ref={dialog} role="dialog" aria-modal="true" aria-labelledby="desktop-permissions-title" className="desktop-permissions-dialog" onCancel={e => { e.preventDefault(); close() }}>
      <h2 id="desktop-permissions-title">{t('desktop.title')}</h2>
      <p>{t('desktop.host', { host: status?.host_name || 'Hexagon' })}</p>
      <div aria-live="polite" aria-busy={busy}>
        {busy && <p>{t('desktop.checking')}</p>}
        {error && <p role="alert">{error}</p>}
        {status && (!status.supported ? <p>{t('desktop.unsupported')}</p> : !status.available ? <><p role="alert">{t('desktop.unavailable')}</p><details><summary>{t('desktop.details')}</summary><pre>{status.error}</pre></details></> : <>
          <p>{t(status.ready ? 'desktop.ready' : 'desktop.missing')}</p>
          <div className="desktop-permission-row"><div><strong>{t('desktop.accessibility')}</strong><p>{t('desktop.accessibilityUse')}</p><span>{t(status.accessibility && status.input_events ? 'desktop.granted' : 'desktop.denied')}</span></div>
            {(!status.accessibility || !status.input_events) && <button className="btn" onClick={() => void settings('accessibility')} title={tip('desktopAccessibility', t('desktop.openAccessibility'))}>{t('desktop.openAccessibility')}</button>}
          </div>
          <div className="desktop-permission-row"><div><strong>{t('desktop.screenRecording')}</strong><p>{t('desktop.screenUse')}</p><span>{t(status.screen_recording ? 'desktop.granted' : 'desktop.denied')}</span></div>
            {!status.screen_recording && <button className="btn" onClick={() => void settings('screen_recording')} title={tip('desktopScreenRecording', t('desktop.openScreen'))}>{t('desktop.openScreen')}</button>}
          </div>
          {!status.ready && <p className="dim3">{t('desktop.path')}</p>}
          <p className="dim3">{t('desktop.separate')}</p>
        </>)}
      </div>
      <div className="desktop-permissions-actions"><button className="btn" disabled={busy} onClick={() => void check()} title={tip('desktopPermissions', t('desktop.recheck'))}>{t('desktop.recheck')}</button><button className="btn primary" onClick={close}>{t('desktop.close')}</button></div>
    </dialog>}
  </>
}
