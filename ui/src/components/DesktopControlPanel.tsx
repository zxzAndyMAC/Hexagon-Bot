import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches, type ActionId } from '../keymap'
import type { DesktopStatus } from '../gen/DesktopStatus'
import type { DesktopControl } from '../gen/DesktopControl'
import { DesktopPermissions } from './DesktopPermissions'
import { Icon } from './Icon'

const controlKeys: Partial<Record<DesktopControl, ActionId>> = {
  enable: 'desktopEnable', disable: 'desktopDisable', pause: 'desktopPause',
  resume: 'desktopResume', release: 'desktopRelease', clear_screenshots: 'desktopClear', prepare_capture: 'desktopPrepareCapture',
}
export function DesktopControlPanel({ inline = false }: { inline?: boolean }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(inline)
  const [status, setStatus] = useState<DesktopStatus | null>(null)
  const [error, setError] = useState('')
  const [pending, setPending] = useState(false)
  const container = useRef<HTMLDivElement>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const mounted = useRef(true)
  const generation = useRef(0)
  const writing = useRef(false)
  const polling = useRef(false)
  const permissionsWarning = useRef(false)
  const refresh = useCallback(async () => {
    if (polling.current || writing.current) return
    polling.current = true
    const ticket = generation.current
    try {
      const value = await api.desktopStatus()
      if (mounted.current && ticket === generation.current) {
        setStatus(value); setError('')
        if (value.enabled) {
          const permissions = await api.desktopPermissions()
          if (mounted.current && ticket === generation.current) {
            if (!permissions.ready && !permissionsWarning.current) window.dispatchEvent(new Event('hexagon:desktop-permissions'))
            permissionsWarning.current = !permissions.ready
          }
        }
      }
    } catch (e) { if (mounted.current && ticket === generation.current) setError(errText(e)) }
    finally { polling.current = false }
  }, [])
  useEffect(() => {
    mounted.current = true
    void refresh()
    const tick = window.setInterval(() => { if (!document.hidden) void refresh() }, 2000)
    window.addEventListener('focus', refresh)
    window.addEventListener('hexagon:desktop-status-changed', refresh)
    return () => { mounted.current = false; generation.current++; window.clearInterval(tick); window.removeEventListener('focus', refresh); window.removeEventListener('hexagon:desktop-status-changed', refresh) }
  }, [refresh])
  const control = useCallback(async (action: DesktopControl) => {
    if (writing.current || !status || ((action === 'resume' || action === 'prepare_capture') && status.capture_preparation === 'preparing')) return
    const projectRoot = status.project_root
    const ticket = ++generation.current
    writing.current = true; setPending(true); setError('')
    try {
      if (action === 'enable') {
        const permissions = await api.desktopPermissions()
        // Ticket 07: a delayed preflight from a closed workspace must never
        // enable the new project or open its permission dialog.
        if (!mounted.current || ticket !== generation.current) return
        if (!permissions.ready) { window.dispatchEvent(new Event('hexagon:desktop-permissions')); return }
      }
      const value = await api.desktopControl(action, projectRoot)
      window.dispatchEvent(new Event('hexagon:desktop-status-changed'))
      if (mounted.current) {
        setStatus(value)
        if (action === 'clear_screenshots') window.dispatchEvent(new Event('hexagon:screenshots-cleared'))
      }
    } catch (e) { if (mounted.current) setError(errText(e)) }
    finally { writing.current = false; if (mounted.current) setPending(false) }
  }, [status])
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return
      if (!inline && matches(event, bindingFor('desktopPanel'))) { event.preventDefault(); setOpen(v => !v); return }
      // The application owns the one global pause listener, including Settings.
      // Other changes require the visible panel to avoid accidental authorization.
      if (!open) return
      for (const [action, id] of Object.entries(controlKeys)) {
        if (id && action !== 'pause' && matches(event, bindingFor(id))) {
          event.preventDefault(); void control(action as DesktopControl); return
        }
      }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [open, control, inline])
  useEffect(() => {
    if (!open) return
    const outside = (event: PointerEvent) => { if (!inline && event.target instanceof Node && !container.current?.contains(event.target)) setOpen(false) }
    document.addEventListener('pointerdown', outside)
    return () => document.removeEventListener('pointerdown', outside)
  }, [open, inline])
  const label = status?.paused ? 'computer.paused' : status?.active_agent ? 'computer.busy' : status?.enabled ? 'computer.enabled' : 'computer.title'
  const button = (action: DesktopControl, disabled = false) => <button type="button" className="btn" disabled={pending || disabled}
    title={`${t(`computer.${action}`)} · ${formatBinding(bindingFor(controlKeys[action]!))}`}
    onClick={() => void control(action)}>{t(`computer.${action}`)}</button>
  return <div className={`desktop-control${inline ? ' desktop-control-inline' : ''}`} ref={container} onKeyDown={event => {
    if (!inline && event.key === 'Escape' && open) { event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus() }
  }}>
    {!inline && <button type="button" className="approval-mode-trigger" ref={trigger} aria-expanded={open} aria-haspopup="dialog"
      title={`${t('computer.title')} · ${formatBinding(bindingFor('desktopPanel'))}`} onClick={() => setOpen(v => !v)}><Icon name="computer" /><span>{t(label)}</span></button>}
    {!inline && status && (status.enabled || status.active_agent) && !status.paused && <button type="button" className="desktop-pause" disabled={pending}
      title={`${t('computer.pause')} · ${formatBinding(bindingFor('desktopPause'))}`} aria-label={t('computer.pause')} onClick={() => void control('pause')}><Icon name="pause" /></button>}
    {open && <section className="desktop-control-popover" role={inline ? 'region' : 'dialog'} aria-label={t('computer.title')}>
      <strong>{t('computer.title')}</strong>
      <p>{t('computer.consent')}</p>
      <p className="dim3">{t('computer.controlHint')}</p>
      {status && <>
        <p>{t(status.enabled ? 'computer.enabled' : 'computer.disabled')}{status.active_agent && ` · ${status.active_agent}`}</p>
        {status.enabled && status.capture_preparation && <p role="status">{t(`computer.capture_${status.capture_preparation}`)}</p>}
        {status.active_project && <p className="dim3" style={{ overflowWrap: 'anywhere' }}>{status.active_project}</p>}
        {status.outcome_unknown && <p role="status">{t('computer.unknown')}</p>}
        <div className="desktop-control-actions">
          {status.enabled ? button('disable') : button('enable')}
          {status.paused ? button('resume', status.busy || status.capture_preparation === 'preparing') : button('pause', !status.enabled && !status.active_agent)}
          {status.enabled && status.capture_preparation !== 'ready' && button('prepare_capture', status.busy || status.capture_preparation === 'preparing')}
          {button('release', status.busy || !status.active_agent)}
        </div>
        <p className="dim3">{t('computer.screenshots', { count: status.screenshot_count })}</p>
        {button('clear_screenshots', status.busy || !status.screenshot_count)}
      </>}
      {!status && !error && <p>{t('computer.loading')}</p>}
      {error && <p role="alert">{error}</p>}
      <DesktopPermissions />
    </section>}
    {!open && <DesktopPermissions compact />}
  </div>
}
