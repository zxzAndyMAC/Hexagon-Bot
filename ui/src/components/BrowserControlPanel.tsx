// 项目浏览器面板（components D19「面板」右块，control#panels；票 ui-redesign 14 换皮）。
// 结构：标题行（globe +「项目浏览器」）→ 两个选项行（托管 Chromium（隔离）「打开」/ 连接我的 Chrome / Edge「连接」）
// → 选「连接」后出橙色警示 inset 块（连接前请了解 + 安装扩展 + 已安装，继续连接）→ 已连接行 + 断开连接。
// 禁用（D19）：电脑操作暂停或停用时「打开」「继续连接」禁用并在块内写明原因；安装引导仍可点。
// 加载：连接中「正在连接…请完成浏览器扩展提示」+ ring。语义与换皮前一致（Owner 2026-10-06 各条见下）。
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import type { BrowserSession } from '../gen/BrowserSession'
import type { BrowserMode } from '../gen/BrowserMode'
import type { DesktopStatus } from '../gen/DesktopStatus'
import { bindingFor, matches } from '../keymap'
import { browserLaunchBlock } from '../computerControl'
import { ActionButton } from './ComputerParts'
import { Icon } from './Icon'
import { Spinner } from '@/components/ui/spinner'
import { TooltipProvider } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

const BLOCK_TEXT = { loading: 'control.blockedLoading', disabled: 'control.blockedDisabled', paused: 'control.blockedPaused' } as const

export function BrowserControlPanel({ inline = false }: { inline?: boolean }) {
  const { t } = useTranslation()
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
  const container = useRef<HTMLElement>(null)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  useEffect(() => {
    // 面板可见即生效（换皮前：弹层打开时）；打开面板本身的 browserPanel 键由头部控制状态 / App 处理。
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return
      for (const action of ['browserManaged', 'browserConnect', 'browserDetach', 'browserInstallExtension', 'browserContinueConnection'] as const) {
        if (matches(event, bindingFor(action))) { event.preventDefault(); container.current?.querySelector<HTMLButtonElement>(`[data-browser-action=${action}]`)?.click(); return }
      }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [])
  useEffect(() => {
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
    return () => { active = false; clearInterval(timer); window.removeEventListener('focus', refresh); window.removeEventListener('hexagon:desktop-status-changed', refresh) }
  }, [])
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
  const block = browserLaunchBlock(desktopStatus)
  const reason = block ? t(BLOCK_TEXT[block]) : undefined
  const connected = Boolean(session?.connected)
  const option = (mode: BrowserMode, title: string, body: string, button: ReactNode) => (
    <div className={cn('flex items-start gap-2.5 rounded-[7px] px-[9px] py-2', (session?.mode === mode && connected) || (mode === 'extension' && extensionSetup && !connected) ? 'bg-hover' : '')}
      data-browser-option={mode}>
      <span aria-hidden className={cn('mt-0.5 size-4 flex-none rounded-full',
        session?.mode === mode && connected ? 'shadow-[inset_0_0_0_5px_var(--accent)]' : 'shadow-[inset_0_0_0_1.5px_var(--line-strong)]')} />
      <div className="min-w-0 flex-1">
        <div className="font-medium">{title}</div>
        <div className="text-[12px] text-ink-3">{body}</div>
      </div>
      {button}
    </div>
  )
  return (
    <TooltipProvider>
      <section ref={container} role="region" aria-label={t('browser.title')} aria-busy={pending} data-browser-panel
        className={cn('flex flex-col text-[13px] text-ink', inline ? '' : 'p-1.5')}>
        <div className="flex items-center gap-2 px-2.5 pt-2.5 pb-2">
          <Icon name="web" size={14} />
          <b className="flex-1 text-[13px] font-semibold">{t('browser.title')}</b>
        </div>
        {option('managed', t('control.managedTitle'), t('control.managedBody'),
          <ActionButton size="xs" label={t('control.open')} action="browserManaged" data-browser-action="browserManaged"
            disabled={pending || !!block || connected} disabledReason={reason} onClick={() => void change('managed')} />)}
        {option('extension', t('control.extensionTitle'), t('control.extensionBody'),
          <ActionButton size="xs" label={t('control.connect')} action="browserConnect" data-browser-action="browserConnect"
            disabled={pending || connected} onClick={() => { setExtensionSetup(true); setError('') }} />)}
        {reason && !connected && <p role="status" className="px-2.5 pt-1 text-[12px] text-ink-2" data-browser-blocked={block}>{reason}</p>}
        {extensionSetup && !connected && (
          <div className="mx-1 my-1.5 rounded-[var(--r-card)] bg-orange-tint p-3" data-browser-setup>
            <div className="flex items-center gap-1.5 text-[13px] font-medium text-orange"><Icon name="warn" size={14} />{t('control.warnTitle')}</div>
            <p className="mt-1 text-[12px] text-ink-2">{t('browser.extensionAccess')}</p>
            <p className="mt-1 text-[12px] text-ink-3">{t('browser.extensionHint')}</p>
            <div className="mt-2 flex gap-1.5">
              <ActionButton size="xs" label={t('browser.installExtension')} action="browserInstallExtension" data-browser-action="browserInstallExtension"
                disabled={openingStore || pending} loading={openingStore} onClick={() => void installExtension()}>
                <Icon name="external" size={12} />{t('browser.installExtension')}
              </ActionButton>
              <ActionButton size="xs" variant="primary" label={t('browser.continueConnection')} action="browserContinueConnection" data-browser-action="browserContinueConnection"
                disabled={pending || !!block} disabledReason={reason} onClick={() => void change('extension')} />
            </div>
          </div>
        )}
        {pending && <p role="status" className="flex items-center gap-1.5 px-2.5 pt-1.5 text-[12px] text-ink-2"><Spinner />{t('browser.connecting')}</p>}
        {session && (
          <div className="flex items-center gap-2 px-1.5 py-1.5">
            <span className="min-w-0 flex-1 truncate pl-1 text-[12px] text-ink-3" title={session.url}>
              {session.connected ? t('control.connected', { target: session.title || session.url }) : t('browser.disconnected')}
            </span>
            {session.connected && <ActionButton size="xs" variant="ghost" label={t('browser.detach')} action="browserDetach" data-browser-action="browserDetach"
              disabled={pending} onClick={() => void change()} />}
          </div>
        )}
        {error && <p role="alert" className="px-2.5 pb-2 text-[12px] text-red">{error}</p>}
      </section>
    </TooltipProvider>
  )
}
