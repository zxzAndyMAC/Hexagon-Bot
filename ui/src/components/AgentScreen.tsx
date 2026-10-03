import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { isBrowserReadBusy } from '../browserRead'
import { bindingFor, formatBinding, matches, type ActionId } from '../keymap'
import { pauseDesktop } from '../desktopPause'
import { useUiStore } from '../store'
import { Icon } from './Icon'
import './AgentScreen.css'

type Target = { key: string; root: string; title: string } & (
  { kind: 'browser'; session: string; tab: string; navigation: number }
  | { kind: 'native'; window: number; process: number; snapshot: string }
)

// Keep an owner's Hide choice when Settings temporarily unmounts the workbench.
// This is a session-local UI preference, never permission or a persisted frame.
const hiddenProjects = new Set<string>()

/** Owner 2026-10-02 / extension14: these frames are local, disposable previews.
 * Hiding stops capture, not the agent. No frame goes to the store, trace or model.
 * A single sequential request avoids a growing queue while the app is busy. */
export function AgentScreen() {
  const { t } = useTranslation()
  const projectEpoch = useUiStore(state => state.projectEpoch)
  const projectRoot = useUiStore(state => state.projectRoot)
  const [targetEpoch, setTargetEpoch] = useState(projectEpoch)
  const [visible, setVisible] = useState(true)
  const [expanded, setExpanded] = useState(false)
  const [source, setSource] = useState<'browser' | 'native'>('browser')
  const [targets, setTargets] = useState<Target[]>([])
  const [paused, setPaused] = useState(false)
  const [frame, setFrame] = useState<{ key: string; url: string; receivedAt: number } | null>(null)
  const [error, setError] = useState('')
  const [position, setPosition] = useState<{ x: number; y: number } | null>(null)
  const [viewport, setViewport] = useState({ width: window.innerWidth, height: window.innerHeight })
  const [pageVisible, setPageVisible] = useState(!document.hidden)
  const mounted = useRef(true)
  const panel = useRef<HTMLElement>(null)
  const reopen = useRef<HTMLButtonElement>(null)
  const root = useRef<string | null>(null)
  const drag = useRef<{ x: number; y: number; left: number; top: number } | null>(null)
  // Issue14 P2: the project API invalidates its epoch before awaiting IPC.
  // Filter during render so old pixels/focus disappear before any host poll.
  const target = targetEpoch === projectEpoch && (projectRoot === null ? projectEpoch === 0 : root.current === projectRoot)
    ? targets.find(value => value.kind === source) ?? targets[0] ?? null : null
  const targetKey = target?.key
  const currentTarget = useRef(target)
  useLayoutEffect(() => { currentTarget.current = target }, [target])
  useLayoutEffect(() => {
    setTargets([]); setFrame(null); setError(''); setPosition(null)
    root.current = null
  }, [projectEpoch])

  useEffect(() => {
    mounted.current = true
    let stopped = false
    let timer: ReturnType<typeof setTimeout>
    const poll = async () => {
      const requestEpoch = projectEpoch
      if (requestEpoch > 0 && projectRoot === null) return
      try {
        const status = await api.desktopStatus()
        if (stopped || useUiStore.getState().projectEpoch !== requestEpoch || (projectRoot !== null && status.project_root !== projectRoot)) return
        if (root.current !== status.project_root) {
          root.current = status.project_root
          setTargets([]); setFrame(null); setError(''); setPosition(null)
          setVisible(!hiddenProjects.has(status.project_root))
        }
        setPaused(status.paused)
        if (!status.enabled) { setTargets([]); setFrame(null); return }
        const [browser, native] = await Promise.allSettled([
          api.browserStatus(status.project_root), api.desktopPreviewTarget(status.project_root),
        ])
        if (stopped || useUiStore.getState().projectEpoch !== requestEpoch || root.current !== status.project_root) return
        const next: Target[] = []
        if (browser.status === 'fulfilled' && browser.value?.connected) {
          const b = browser.value
          next.push({ kind: 'browser', root: status.project_root, session: b.session_id, tab: b.tab_id,
            navigation: b.navigation_generation, title: b.title || b.url,
            key: JSON.stringify([status.project_root, b.session_id, b.tab_id, b.navigation_generation]) })
        }
        // Issue19: a historical native receipt is not an active computer session.
        if (status.active_agent && status.active_project === status.project_root && native.status === 'fulfilled' && native.value) {
          const n = native.value
          next.push({ kind: 'native', root: status.project_root, window: n.window_id, process: n.process_id,
            snapshot: n.snapshot_id, title: n.title || n.bundle_id || '', key: JSON.stringify([status.project_root, n.window_id, n.process_id, n.snapshot_id]) })
        }
        setTargetEpoch(requestEpoch); setTargets(next)
      } catch { if (!stopped) { setTargets([]); setFrame(null) } }
      finally { if (!stopped) timer = setTimeout(poll, 1000) }
    }
    void poll()
    const visibility = () => { setPageVisible(!document.hidden); if (document.hidden) setFrame(null) }
    const resize = () => setViewport({ width: window.innerWidth, height: window.innerHeight })
    document.addEventListener('visibilitychange', visibility)
    window.addEventListener('resize', resize)
    return () => {
      stopped = true; mounted.current = false; clearTimeout(timer)
      document.removeEventListener('visibilitychange', visibility)
      window.removeEventListener('resize', resize)
    }
  }, [projectEpoch, projectRoot])

  useEffect(() => {
    if (!visible || !pageVisible || !target) return
    let stopped = false
    let timer: ReturnType<typeof setTimeout>
    const selected = target
    const selectedEpoch = projectEpoch
    const poll = async () => {
      try {
        const value = selected.kind === 'browser'
          ? await api.browserPreview(selected.root, selected.session)
          : await api.desktopPreview(selected.root, selected.window, selected.process)
        if (stopped || useUiStore.getState().projectEpoch !== selectedEpoch || root.current !== selected.root || currentTarget.current?.key !== selected.key) return
        // Session navigation changes can happen while a screenshot is pending.
        // Never display those pixels under the old page label.
        if (selected.kind === 'browser' && (!('session_id' in value) ||
          value.session_id !== selected.session || value.tab_id !== selected.tab || value.navigation_generation !== selected.navigation)) {
          setFrame(null); return
        }
        if (selected.kind === 'native' && (!('window_id' in value) ||
          value.window_id !== selected.window || value.process_id !== selected.process || value.snapshot_id !== selected.snapshot)) {
          setFrame(null); return
        }
        setFrame({ key: selected.key, url: value.data_url, receivedAt: Date.now() }); setError('')
      } catch (e) {
        if (!stopped) {
          if (selected.kind === 'browser' && isBrowserReadBusy(e)) {
            // Picker polling once blinked the whole preview on each contention.
            // Bridge a short scheduling miss, never display an indefinitely old
            // frame; identity/revocation/hide gates above remain unconditional.
            setFrame(previous => previous?.key === selected.key && Date.now() - previous.receivedAt < 1500 ? previous : null)
            setError('')
          } else { setFrame(null); setError(errText(e)) }
        }
      } finally { if (!stopped) timer = setTimeout(poll, 500) }
    }
    void poll()
    return () => {
      stopped = true; clearTimeout(timer)
      if (selected.kind === 'native') void api.desktopPreviewStop(selected.root).catch(() => {})
    }
    // Identity is the dependency; a metadata poll must not restart capture.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, pageVisible, targetKey, projectEpoch])

  const hide = useCallback(() => { if (root.current) hiddenProjects.add(root.current); setVisible(false); setFrame(null); setError(''); queueMicrotask(() => reopen.current?.focus()) }, [])
  const show = useCallback(() => { if (root.current) hiddenProjects.delete(root.current); setVisible(true) }, [])
  const focusTarget = useCallback(async () => {
    const selected = currentTarget.current
    if (!selected || useUiStore.getState().projectEpoch !== targetEpoch) return
    try {
      if (selected.kind === 'browser') await api.browserFocus(selected.root, selected.session)
      else await api.desktopPreviewFocus(selected.root, selected.window, selected.process)
    } catch (e) { if (mounted.current && root.current === selected.root) useUiStore.getState().pushToast(errText(e), 'err') }
  }, [targetEpoch])
  const resume = async () => {
    const selected = currentTarget.current
    if (!selected || useUiStore.getState().projectEpoch !== targetEpoch) return
    try {
      const status = await api.desktopControl('resume', selected.root)
      if (mounted.current && root.current === selected.root) { setPaused(status.paused); setError('') }
    } catch (e) { if (mounted.current && root.current === selected.root) useUiStore.getState().pushToast(errText(e), 'err') }
  }
  useEffect(() => {
    const keys = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return
      if (matches(e, bindingFor('agentScreen'))) { e.preventDefault(); if (visible) hide(); else show(); return }
      if (!visible || !currentTarget.current) return
      if (matches(e, bindingFor('agentScreenExpand'))) { e.preventDefault(); setExpanded(v => !v) }
      if (matches(e, bindingFor('agentScreenHide'))) { e.preventDefault(); hide() }
      if (matches(e, bindingFor('agentScreenFocus'))) { e.preventDefault(); void focusTarget() }
      if (matches(e, bindingFor('agentScreenMove'))) { e.preventDefault(); panel.current?.querySelector<HTMLButtonElement>('.agent-screen-drag')?.focus() }
      if (matches(e, bindingFor('agentScreenSource'))) { e.preventDefault(); panel.current?.querySelector('select')?.focus() }
    }
    window.addEventListener('keydown', keys)
    return () => window.removeEventListener('keydown', keys)
  }, [visible, hide, show, focusTarget])
  const tip = (action: ActionId, label: string) => [label, formatBinding(bindingFor(action))].filter(Boolean).join(' · ')
  const width = Math.min(expanded ? 560 : 268, Math.max(180, viewport.width - 24))
  const height = expanded ? 500 : 310
  const clamp = (x: number, y: number) => ({ x: Math.max(12, Math.min(x, viewport.width - width - 12)), y: Math.max(12, Math.min(y, viewport.height - height - 12)) })
  const location = clamp(position?.x ?? viewport.width - width - 26, position?.y ?? viewport.height - height - 110)
  const shownFrame = visible && pageVisible && frame && frame.key === targetKey ? frame.url : null
  // Owner issue19: an unavailable target must not leave a permanent empty control.
  if (!target) return null
  if (!visible) return <button ref={reopen} type="button" className="agent-screen-reopen"
    disabled={!target} title={tip('agentScreen', t(target ? 'agentScreen.title' : 'agentScreen.unavailable'))} onClick={show}><Icon name="computer" />{t('agentScreen.title')}</button>
  return <aside ref={panel} className={`agent-screen${expanded ? ' agent-screen-expanded' : ''}`} aria-label={t('agentScreen.title')}
    style={{ left: location.x, top: location.y, width }}>
    <header>
      <button type="button" className="agent-screen-drag" aria-label={t('agentScreen.move')} title={tip('agentScreenMove', t('agentScreen.move'))}
        onPointerDown={e => { if (e.button !== 0) return; drag.current = { x: e.clientX, y: e.clientY, left: location.x, top: location.y }; e.currentTarget.setPointerCapture?.(e.pointerId) }}
        onPointerMove={e => { if (drag.current) setPosition(clamp(drag.current.left + e.clientX - drag.current.x, drag.current.top + e.clientY - drag.current.y)) }}
        onPointerUp={e => { drag.current = null; e.currentTarget.releasePointerCapture?.(e.pointerId) }} onPointerCancel={() => { drag.current = null }}
        onKeyDown={e => { const delta = { ArrowLeft: [-20, 0], ArrowRight: [20, 0], ArrowUp: [0, -20], ArrowDown: [0, 20] }[e.key]; if (delta) { e.preventDefault(); setPosition(clamp(location.x + delta[0], location.y + delta[1])) } }}>
        <Icon name="computer" /><strong>{t('agentScreen.title')}</strong>
      </button>
      <button type="button" onClick={hide} title={tip('agentScreenHide', t('agentScreen.hide'))} aria-label={t('agentScreen.hide')}>×</button>
    </header>
    {targets.length > 1 && <select aria-label={t('agentScreen.source')} title={tip('agentScreenSource', t('agentScreen.source'))} value={target.kind} onChange={e => { setSource(e.target.value as 'browser' | 'native'); setFrame(null); setError('') }}>
      <option value="browser">{t('agentScreen.browser')}</option><option value="native">{t('agentScreen.native')}</option>
    </select>}
    <button type="button" className="agent-screen-picture" title={tip('agentScreenExpand', t(expanded ? 'agentScreen.collapse' : 'agentScreen.expand'))}
      onClick={() => setExpanded(v => !v)}>
      {shownFrame ? <img src={shownFrame} alt={target.title} decoding="async" /> : <span role="status">{t(error ? 'agentScreen.unavailable' : 'agentScreen.loading')}</span>}
    </button>
    <div className="agent-screen-caption"><span title={target.title}>{target.title}</span><span>{t(paused ? 'computer.paused' : 'agentScreen.local')}</span></div>
    <footer>
      <button type="button" className="btn" onClick={() => void focusTarget()} title={tip('agentScreenFocus', t('agentScreen.open'))}>{t('agentScreen.open')}</button>
      <button type="button" className="btn" onClick={() => void (paused ? resume() : pauseDesktop())}
        onKeyDown={e => { if (paused && matches(e.nativeEvent, bindingFor('desktopResume'))) { e.preventDefault(); e.stopPropagation(); void resume() } }}
        title={tip(paused ? 'desktopResume' : 'desktopPause', t(paused ? 'computer.resume' : 'computer.pause'))}>{t(paused ? 'computer.resume' : 'computer.pause')}</button>
    </footer>
  </aside>
}
