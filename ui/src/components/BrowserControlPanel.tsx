import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import type { BrowserSession } from '../gen/BrowserSession'
import type { BrowserMode } from '../gen/BrowserMode'
import type { DesktopStatus } from '../gen/DesktopStatus'
import { bindingFor, formatBinding, matches, type ActionId } from '../keymap'
import { Icon } from './Icon'

export function BrowserControlPanel({ inline = false }: { inline?: boolean }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(inline)
  const [session, setSession] = useState<BrowserSession | null>(null)
  const [root, setRoot] = useState('')
  const [desktopStatus, setDesktopStatus] = useState<DesktopStatus | null>(null)
  const [pending, setPending] = useState(false)
  const [extensionSetup, setExtensionSetup] = useState(false)
  const [openingStore, setOpeningStore] = useState(false)
  const [error, setError] = useState('')
  const mounted = useRef(true)
  const writing = useRef(false)
  const generation = useRef(0)
  const trigger = useRef<HTMLButtonElement>(null)
  const container = useRef<HTMLDivElement>(null)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return
      if (!inline && matches(event, bindingFor('browserPanel'))) { event.preventDefault(); setOpen(value => !value); return }
      if (open) for (const action of ['browserManaged', 'browserConnect', 'browserDetach', 'browserInstallExtension', 'browserContinueConnection'] as const) {
        if (matches(event, bindingFor(action))) { event.preventDefault(); container.current?.querySelector<HTMLButtonElement>(`[data-browser-action=${action}]`)?.click(); return }
      }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [open, inline])
  useEffect(() => {
    if (!open) return
    let active = true, polling = false
    const refresh = async () => {
      if (polling || writing.current) return
      polling = true
      const ticket = generation.current
      let consentRead = false
      try {
        const desktop = await api.desktopStatus()
        if (!active || !mounted.current || ticket !== generation.current) return
        // Owner 2026-10-06: a disconnected browser read once discarded valid
        // consent/pause state and permanently greyed both explicit retry paths.
        setRoot(desktop.project_root); setDesktopStatus(desktop)
        consentRead = true
        setSession(previous => previous?.project_root === desktop.project_root ? previous : null)
        const browser = await api.browserStatus(desktop.project_root)
        if (active && mounted.current && ticket === generation.current) {
          setSession(browser); setError('')
        }
      } catch (e) {
        if (active && mounted.current && ticket === generation.current) {
          if (!consentRead) { setRoot(''); setDesktopStatus(null) }
          setError(errText(e))
        }
      }
      finally { polling = false }
    }
    void refresh()
    const timer = window.setInterval(() => { if (!document.hidden) void refresh() }, 2000)
    window.addEventListener('focus', refresh)
    window.addEventListener('hexagon:desktop-status-changed', refresh)
    const outside = (event: PointerEvent) => { if (!inline && event.target instanceof Node && !container.current?.contains(event.target)) setOpen(false) }
    document.addEventListener('pointerdown', outside)
    return () => { active = false; clearInterval(timer); document.removeEventListener('pointerdown', outside); window.removeEventListener('focus', refresh); window.removeEventListener('hexagon:desktop-status-changed', refresh) }
  }, [open, inline])
  async function change(mode?: BrowserMode) {
    // Owner 2026-10-06: consent reads may fail while a known session is live.
    // Keep explicit detach bound to that session; only launches need fresh consent.
    const projectRoot = mode ? root : session?.project_root
    if (!projectRoot || writing.current) return
    writing.current = true; setPending(true); setError('')
    const ticket = ++generation.current
    try {
      if (mode) {
        const result = await api.browserOpen(projectRoot, mode, { element: t('elementContext.select'), region: t('elementContext.region'), done: t('elementContext.done'), hint: t('elementContext.hint') })
        if (mounted.current && ticket === generation.current) setSession(result)
      } else if (session) {
        let disconnected: BrowserSession | null = null
        try { await api.browserDetach(projectRoot, session.session_id) }
        catch (error) {
          // Owner 2026-10-06: manually closing Chromium exits its worker before
          // the next UI poll. A liveness rejection then looked like an internal
          // detach failure. Reconcile only a host-confirmed closed/absent session;
          // preserve real failures and any replacement session's identity.
          try { disconnected = await api.browserStatus(projectRoot) }
          catch { throw error }
          if (disconnected && (disconnected.connected || disconnected.project_root !== projectRoot || disconnected.session_id !== session.session_id)) throw error
        }
        if (mounted.current && ticket === generation.current) setSession(disconnected)
      }
    } catch (e) { if (mounted.current && ticket === generation.current) setError(errText(e)) }
    finally { writing.current = false; if (mounted.current) setPending(false) }
  }
  async function installExtension() {
    if (openingStore) return
    setOpeningStore(true); setError('')
    try { await api.openBrowserExtensionStore() }
    catch (error) { if (mounted.current) setError(errText(error)) }
    finally { if (mounted.current) setOpeningStore(false) }
  }
  const tip = (key: string, action: ActionId) => [t(key), formatBinding(bindingFor(action))].filter(Boolean).join(' · ')
  const enabled = desktopStatus?.enabled && !desktopStatus.paused
  return <div ref={container} className={`desktop-control${inline ? ' desktop-control-inline' : ''}`} onKeyDown={event => {
    if (!inline && event.key === 'Escape' && open) { event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus() }
  }}>
    {!inline && <button ref={trigger} type="button" className="approval-mode-trigger" aria-expanded={open} aria-haspopup="dialog"
      title={tip('browser.title', 'browserPanel')} onClick={() => setOpen(value => !value)}><Icon name="web" />{t('browser.title')}</button>}
    {open && <section className="desktop-control-popover" role={inline ? 'region' : 'dialog'} aria-label={t('browser.title')} aria-busy={pending}>
      <strong>{t('browser.title')}</strong><p>{t('browser.description')}</p>
      <p className="dim3">{t('browser.extensionHint')}</p>
      {!desktopStatus && <p>{t('computer.loading')}</p>}
      {desktopStatus && !enabled && <p role="status">{t(desktopStatus.enabled ? 'computer.paused' : 'computer.disabled')} · {t('browser.enableHint')}</p>}
      {session && <p>{t(session.mode === 'managed' ? 'browser.managed' : 'browser.extension')} · {session.connected ? session.title || session.url : t('browser.disconnected')}</p>}
      <div className="desktop-control-actions">
        <button data-browser-action="browserManaged" className="btn" disabled={pending || !enabled || Boolean(session?.connected)} title={tip('browser.managed', 'browserManaged')} onClick={() => void change('managed')}>{t('browser.managed')}</button>
        <button data-browser-action="browserConnect" className="btn" disabled={pending || Boolean(session?.connected)} title={tip('browser.extension', 'browserConnect')} onClick={() => { setExtensionSetup(true); setError('') }}>{t('browser.extension')}</button>
        {session?.connected && <button data-browser-action="browserDetach" className="btn" disabled={pending} title={tip('browser.detach', 'browserDetach')} onClick={() => void change()}>{t('browser.detach')}</button>}
      </div>
      {extensionSetup && !session?.connected && <div className="browser-extension-setup">
        <strong>{t('browser.setupTitle')}</strong>
        <p>{t('browser.setupBody')}</p>
        <p>{t('browser.extensionAccess')}</p>
        <div className="desktop-control-actions">
          <button className="btn" data-browser-action="browserInstallExtension" disabled={openingStore || pending}
            title={tip('browser.installExtension', 'browserInstallExtension')} onClick={() => void installExtension()}>{t('browser.installExtension')}</button>
          <button className="btn" data-browser-action="browserContinueConnection" disabled={pending || !enabled}
            title={tip('browser.continueConnection', 'browserContinueConnection')} onClick={() => void change('extension')}>{t('browser.continueConnection')}</button>
        </div>
      </div>}
      {pending && <p role="status">{t('browser.connecting')}</p>}
      {error && <p role="alert">{error}</p>}
    </section>}
  </div>
}
