import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { isBrowserReadBusy } from '../browserRead'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { ElementRef } from '../gen/ElementRef'
import type { BrowserSession } from '../gen/BrowserSession'
import { useUiStore } from '../store'
import { blockBackgroundDecision } from '../modalDecisionGuard'
import { Icon } from './Icon'
import { buttonVariants } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import './ElementReferences.css'
import { DesktopToolDetails } from './DesktopToolDetails'

export function ElementReference({ reference, stale = false }: { reference: ElementRef; stale?: boolean }) {
  const { t } = useTranslation()
  return <div className="panel element-reference" style={{ padding: '6px 10px', minWidth: 0 }}>
    <div><strong>{t(reference.kind === 'region' ? 'elementContext.region' : 'elementContext.element')}</strong>{reference.tag && ` · ${reference.tag}`}
      {stale && <span className="chip warn">{t('elementContext.stale')}</span>}</div>
    <div className="dim3" style={{ fontSize: 10, overflowWrap: 'anywhere' }}>{reference.url}</div>
    {reference.text && <p style={{ margin: '4px 0', maxHeight: 64, overflow: 'auto', whiteSpace: 'pre-wrap' }}>{reference.text}</p>}
    <div className="dim3" style={{ fontSize: 10 }}>{reference.source_hint
      ? t('elementContext.sourceHint', { file: reference.source_hint.file, line: reference.source_hint.line }) : t('elementContext.unlocated')}</div>
    <DesktopToolDetails projectId={reference.project_root} capturedAt={new Date(reference.captured_at_ms).toISOString()}
      output={{ result: { screenshot_path: `${reference.project_root}/.hexagon/computer-use/screenshots/${reference.screenshot}`, title: reference.url } }} />
  </div>
}

/** A local draft inbox, never a send trigger. Owner message persistence alone
 * consumes the host-minted IDs; navigating cannot retarget an existing chip. */
/** ui-redesign 票 07（components D1 / D3）：输入条把「选择网页元素」放进输入条上方的工具行、
 * 引用卡放进输入条内的引用区，两处共用这一份轮询与草稿状态——传 children 渲染函数取各段；
 * 不传时按旧版一列渲染（工具在上、引用在下）。 */
export type ElementDraftParts = { tools: ReactNode; list: ReactNode; preview: ReactNode; status: ReactNode }

export function ElementDraft({ references, onChange, submitting, trailing, children }: {
  references: ElementRef[]; onChange: React.Dispatch<React.SetStateAction<ElementRef[]>>; submitting: boolean; trailing?: React.ReactNode
  children?: (parts: ElementDraftParts) => ReactNode
}) {
  const { t } = useTranslation()
  const [session, setSession] = useState<BrowserSession | null>(null)
  const [error, setError] = useState('')
  const [starting, setStarting] = useState(false)
  const [inspected, setInspected] = useState<string | null>(null)
  const root = useRef<string | null>(null)
  const epoch = useRef(0)
  const mounted = useRef(true)
  const drafts = useRef(references)
  const sending = useRef(submitting)
  useLayoutEffect(() => { drafts.current = references; sending.current = submitting }, [references, submitting])
  useEffect(() => {
    mounted.current = true
    let running = false
    const refresh = async () => {
      if (running || sending.current) return
      running = true
      const generation = epoch.current
      try {
        const desktop = await api.desktopStatus()
        if (!mounted.current || generation !== epoch.current) return
        if (root.current && root.current !== desktop.project_root) { root.current = desktop.project_root; epoch.current++; onChange([]); setSession(null); setStarting(false); return }
        root.current = desktop.project_root
        if (!desktop.enabled) { setSession(null); return }
        const current = await api.browserStatus(desktop.project_root)
        if (!mounted.current || generation !== epoch.current) return
        setSession(current)
        setError('')
        if (!current?.connected) return
        const picked = await api.browserSelectionPoll(current.project_root, current.session_id)
        if (!mounted.current || generation !== epoch.current || sending.current) return
        if (picked.length) {
          const fresh = picked.filter(item => !drafts.current.some(old => old.id === item.id))
          const room = Math.max(0, 8 - drafts.current.length)
          onChange(previous => [...previous, ...fresh.slice(0, room)])
          if (fresh.length > room) void api.browserSelectionDiscard(current.project_root, fresh.slice(room).map(item => item.id)).catch(() => {})
        }
      } catch (error) {
        if (mounted.current && generation === epoch.current && !isBrowserReadBusy(error)) setError(errText(error))
      } finally { running = false }
    }
    const cleared = () => { epoch.current++; onChange([]); setStarting(false) }
    void refresh()
    const timer = setInterval(() => void refresh(), 1200)
    window.addEventListener('hexagon:screenshots-cleared', cleared)
    return () => { mounted.current = false; epoch.current++; clearInterval(timer); window.removeEventListener('hexagon:screenshots-cleared', cleared) }
  }, [onChange])
  const start = useCallback(async () => {
    if (!session?.connected || submitting || starting) return
    const generation = epoch.current
    setStarting(true); setError('')
    try {
      await api.browserSelectionStart(session.project_root, session.session_id, {
        element: t('elementContext.select'), region: t('elementContext.region'), done: t('elementContext.done'), hint: t('elementContext.hint'),
      })
    } catch (error) { if (mounted.current && generation === epoch.current) setError(errText(error)) }
    finally { if (mounted.current && generation === epoch.current) setStarting(false) }
  }, [session, submitting, starting, t])
  useEffect(() => {
    const key = (event: KeyboardEvent) => { if (!event.defaultPrevented && matches(event,bindingFor('selectBrowserElement'))) { event.preventDefault(); void start() } }
    window.addEventListener('keydown',key); return () => window.removeEventListener('keydown',key)
  }, [start])
  const remove = (reference: ElementRef) => {
    if (submitting) return
    onChange(previous => previous.filter(item => item.id !== reference.id))
    void api.browserSelectionDiscard(reference.project_root,[reference.id]).catch(() => {})
  }
  const isStale = (reference: ElementRef) => !session || session.session_id !== reference.session_id
    || session.tab_id !== reference.tab_id || session.navigation_generation !== reference.navigation_generation
  // components D3：选择中工具行按钮换「选择中… 点击页面元素」+「已连接 · host」
  const host = (() => { try { return session?.url ? new URL(session.url).host : '' } catch { return '' } })()
  const tools = session?.connected ? <>
    {/* 原生 disabled（不是 Button 的 aria-disabled）：发送中 / 开启中这颗钮确实不可再点，测试按 .disabled 断言 */}
    <button type="button" className={cn(buttonVariants({ size: 'xs' }), 'disabled:cursor-default',
      starting && 'text-accent-ink [--hx-shadow:0_0_0_1px_var(--accent),var(--shadow-xs)]', submitting && !starting && 'opacity-45')}
      disabled={submitting || starting} onClick={() => void start()}
      title={`${t('elementContext.select')} · ${formatBinding(bindingFor('selectBrowserElement'))}`}>
      <Icon name="crosshair" size={12} />{t(starting ? 'elementContext.selecting' : 'elementContext.select')}
    </button>
    {starting && host && <span className="host">{t('elementContext.connectedHost', { host })}</span>}
  </> : null
  const status = error ? <span role="status" className="element-draft-status">{error}</span> : null
  const list = references.length > 0 ? <div className="element-draft-list">
      {references.map(reference => <div className="element-draft-item" key={reference.id} onKeyDown={event => {
        if (matches(event.nativeEvent,bindingFor('removeBrowserElement'))) { event.preventDefault(); event.stopPropagation(); remove(reference) }
      }}>
        <button className="element-draft-open" type="button" aria-label={`${t('elementContext.view')}: ${reference.text || reference.tag || reference.url}`}
          title={[t('elementContext.view'),formatBinding(bindingFor('viewBrowserElement'))].filter(Boolean).join(' · ')}
          onClick={() => setInspected(reference.id)} onKeyDown={event => {
            if (matches(event.nativeEvent,bindingFor('viewBrowserElement'))) { event.preventDefault(); setInspected(reference.id) }
          }}>
          <span className="eref-th"><Icon name={reference.kind === 'region' ? 'image' : 'crosshair'} size={14} /></span>
          <span className="element-draft-copy"><strong>{referenceTitle(reference, t)}</strong>
            {isStale(reference) ? <span data-stale="">{t('elementContext.stale')}</span>
              : <span>{reference.source_hint ? `${reference.source_hint.file}:${reference.source_hint.line}` : t('elementContext.unlocated')}</span>}</span>
        </button>
        <button className="element-draft-remove" type="button" disabled={submitting} onClick={() => remove(reference)} aria-label={t('elementContext.remove')}
          title={[t('elementContext.remove'),formatBinding(bindingFor('removeBrowserElement'))].filter(Boolean).join(' · ')}><Icon name="close" size={12} /></button>
      </div>)}
    </div> : null
  const preview = references.filter(reference => reference.id === inspected).map(reference => <ElementReferencePreview key={reference.id} reference={reference}
      stale={isStale(reference)} onClose={() => setInspected(null)} />)
  if (children) return <>{children({ tools, list, preview, status })}</>
  return <div className="element-draft">
    <div className="element-draft-tools">{tools}{trailing}</div>
    {status}
    {list}
    {preview}
  </div>
}

/** components D3 引用卡标题：元素 = 标签「文字」，区域 = 「区域 宽×高」。 */
function referenceTitle(reference: ElementRef, t: (key: string, options?: Record<string, unknown>) => string) {
  if (reference.kind === 'region') return t('elementContext.regionSize', { w: Math.round(reference.rect.width), h: Math.round(reference.rect.height) })
  if (reference.tag && reference.text) return t('elementContext.titled', { tag: reference.tag, text: reference.text })
  return reference.text || reference.tag || t('elementContext.element')
}

/** Owner issue16: full values remain keyboard-accessible instead of relying on
 * hover-only titles for clipped chips. Native top layer isolates the workbench. */
function ElementReferencePreview({ reference, stale, onClose }: { reference: ElementRef; stale: boolean; onClose: () => void }) {
  const { t } = useTranslation()
  const dialog = useRef<HTMLDialogElement>(null)
  const close = useRef<HTMLButtonElement>(null)
  const callback = useRef(onClose)
  useLayoutEffect(() => { callback.current = onClose }, [onClose])
  useEffect(() => {
    const previous = document.activeElement
    const modal = dialog.current
    if (modal?.showModal) modal.showModal(); else modal?.setAttribute('open', '')
    close.current?.focus()
    const key = (event: KeyboardEvent) => {
      if (matches(event,bindingFor('closeBrowserElement'))) {
        event.preventDefault(); event.stopImmediatePropagation(); callback.current()
      } else if (blockBackgroundDecision(event)) {
        useUiStore.getState().pushToast(t('decisions.scopeBlocked'))
      }
    }
    window.addEventListener('keydown',key,true)
    return () => {
      window.removeEventListener('keydown',key,true)
      if (modal?.open && modal.close) modal.close()
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus()
    }
  }, [t])
  return <dialog ref={dialog} className="element-reference-preview" aria-label={t('elementContext.view')}
    onCancel={event => { event.preventDefault(); onClose() }}>
    <header><strong>{t('elementContext.view')}</strong><button className="btn" ref={close} type="button" onClick={onClose}
      title={[t('elementContext.close'),formatBinding(bindingFor('closeBrowserElement')) || 'Esc'].join(' · ')}>{t('elementContext.close')}</button></header>
    <ElementReference reference={reference} stale={stale} />
  </dialog>
}
