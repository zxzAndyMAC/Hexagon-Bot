import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { isBrowserReadBusy } from '../browserRead'
import { bindingFor, matches } from '../keymap'
import { useUiStore } from '../store'
import { Icon } from './Icon'
import { Badge } from '@/components/ui/badge'
import { IconButton } from '@/components/ui/icon-button'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { Spinner } from '@/components/ui/spinner'
import { TooltipProvider } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

type Target = { key: string; root: string; title: string } & (
  { kind: 'browser'; session: string; tab: string; navigation: number }
  | { kind: 'native'; window: number; process: number; snapshot: string }
)

// Keep an owner's Hide choice when Settings temporarily unmounts the workbench.
// This is a session-local UI preference, never permission or a persisted frame.
const hiddenProjects = new Set<string>()

// Agent Screen 浮窗（components D19，control#screen；票 ui-redesign 14 换皮）：300 宽、--r-panel、overlay 阴影、padding 8；
// 画面 16:10 圆角 9；标签行「Agent 的屏幕」+ 工作中 / 已暂停 chip + 打开真实应用 / 展开 / 隐藏 ibtn sm；标签行可拖动
// （拖动中 opacity .9；accessibility.md：拖动的替代是展开 / 隐藏与方向键移动）。暂停电脑操作在头部（D19），浮窗不再重复。
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
  const [dragging, setDragging] = useState(false)
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

  // 票 14：隐藏后焦点回到头部「屏幕」按钮（输入条旁的「显示 Agent Screen」重复入口已撤，ADR 0088 偏差 9 收口）。
  const hide = useCallback(() => {
    if (root.current) hiddenProjects.add(root.current)
    setVisible(false); setFrame(null); setError('')
    queueMicrotask(() => document.querySelector<HTMLButtonElement>('[data-header-screen]')?.focus())
  }, [])
  const show = useCallback(() => { if (root.current) hiddenProjects.delete(root.current); setVisible(true) }, [])
  // ui-redesign 票 02（components B5）：头部「屏幕」按钮只在有目标时渲染，点它与 agentScreen 键同效。
  const hasTarget = target != null
  useEffect(() => {
    useUiStore.setState({ agentScreenAvailable: hasTarget })
    return () => useUiStore.setState({ agentScreenAvailable: false })
  }, [hasTarget])
  const screenReq = useUiStore(state => state.agentScreenReq)
  const lastReq = useRef(screenReq)
  useEffect(() => {
    if (screenReq === lastReq.current) return
    lastReq.current = screenReq
    if (visible) hide(); else show()
  }, [screenReq, visible, hide, show])
  const focusTarget = useCallback(async () => {
    const selected = currentTarget.current
    if (!selected || useUiStore.getState().projectEpoch !== targetEpoch) return
    try {
      if (selected.kind === 'browser') await api.browserFocus(selected.root, selected.session)
      else await api.desktopPreviewFocus(selected.root, selected.window, selected.process)
    } catch (e) { if (mounted.current && root.current === selected.root) useUiStore.getState().pushToast(errText(e), 'err') }
  }, [targetEpoch])
  useEffect(() => {
    const keys = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return
      if (matches(e, bindingFor('agentScreen'))) { e.preventDefault(); if (visible) hide(); else show(); return }
      if (!visible || !currentTarget.current) return
      if (matches(e, bindingFor('agentScreenExpand'))) { e.preventDefault(); setExpanded(v => !v) }
      if (matches(e, bindingFor('agentScreenHide'))) { e.preventDefault(); hide() }
      if (matches(e, bindingFor('agentScreenFocus'))) { e.preventDefault(); void focusTarget() }
      if (matches(e, bindingFor('agentScreenMove'))) { e.preventDefault(); panel.current?.querySelector<HTMLButtonElement>('[data-screen-drag]')?.focus() }
      if (matches(e, bindingFor('agentScreenSource'))) { e.preventDefault(); panel.current?.querySelector<HTMLButtonElement>('[data-screen-source] button')?.focus() }
    }
    window.addEventListener('keydown', keys)
    return () => window.removeEventListener('keydown', keys)
  }, [visible, hide, show, focusTarget])
  // D19：浮窗 300 宽（展开 560），默认锚点头部下方右上角（right 20 · top 64，control#screen）。
  const width = Math.min(expanded ? 560 : 300, Math.max(180, viewport.width - 24))
  const height = Math.round((width - 16) * 10 / 16) + 52 + (targets.length > 1 ? 34 : 0)
  const clamp = (x: number, y: number) => ({ x: Math.max(12, Math.min(x, viewport.width - width - 12)), y: Math.max(12, Math.min(y, viewport.height - height - 12)) })
  const location = clamp(position?.x ?? viewport.width - width - 20, position?.y ?? 64)
  const shownFrame = visible && pageVisible && frame && frame.key === targetKey ? frame.url : null
  // Owner issue19: an unavailable target must not leave a permanent empty control.
  // 票 14：隐藏时不再留输入条旁的重开按钮——头部「屏幕」按钮（有目标才渲染）是唯一重开入口。
  if (!target || !visible) return null
  return <TooltipProvider>
    <aside ref={panel} aria-label={t('control.screenTitle')} data-agent-screen data-expanded={expanded || undefined}
      className={cn('fixed z-40 rounded-[var(--r-panel)] bg-surface p-2 text-ink shadow-[var(--shadow-overlay)]', dragging && 'opacity-90')}
      style={{ left: location.x, top: location.y, width }}>
      <button type="button" data-screen-picture onClick={() => setExpanded(v => !v)}
        aria-label={t(expanded ? 'agentScreen.collapse' : 'agentScreen.expand')}
        className={cn('hx-focus grid w-full place-items-center overflow-hidden rounded-[9px] bg-inset text-[12px] text-ink-3 aspect-[16/10]', expanded ? 'cursor-zoom-out' : 'cursor-zoom-in')}>
        {shownFrame ? <img src={shownFrame} alt={target.title} decoding="async" className="size-full object-contain" />
          : <span role="status" className="inline-flex items-center gap-1.5">{!error && <Spinner />}{t(error ? 'agentScreen.unavailable' : 'agentScreen.loading')}</span>}
      </button>
      {targets.length > 1 && <div className="px-1 pt-2" data-screen-source>
        <Segmented aria-label={t('agentScreen.source')} value={target.kind} onValueChange={v => { setSource(v as 'browser' | 'native'); setFrame(null); setError('') }}>
          <SegmentedItem value="browser">{t('agentScreen.browser')}</SegmentedItem>
          <SegmentedItem value="native">{t('agentScreen.native')}</SegmentedItem>
        </Segmented>
      </div>}
      <div className="flex items-center gap-1.5 px-1 pt-2 pb-0.5">
        <button type="button" data-screen-drag aria-label={t('agentScreen.move')} title={`${target.title} · ${t('agentScreen.move')}`}
          className="hx-focus flex min-w-0 flex-1 cursor-grab touch-none items-center gap-1.5 rounded-[var(--r-xs)] text-left active:cursor-grabbing"
          onPointerDown={e => { if (e.button !== 0) return; drag.current = { x: e.clientX, y: e.clientY, left: location.x, top: location.y }; setDragging(true); e.currentTarget.setPointerCapture?.(e.pointerId) }}
          onPointerMove={e => { if (drag.current) setPosition(clamp(drag.current.left + e.clientX - drag.current.x, drag.current.top + e.clientY - drag.current.y)) }}
          onPointerUp={e => { drag.current = null; setDragging(false); e.currentTarget.releasePointerCapture?.(e.pointerId) }}
          onPointerCancel={() => { drag.current = null; setDragging(false) }}
          onKeyDown={e => { const delta = { ArrowLeft: [-20, 0], ArrowRight: [20, 0], ArrowUp: [0, -20], ArrowDown: [0, 20] }[e.key]; if (delta) { e.preventDefault(); setPosition(clamp(location.x + delta[0], location.y + delta[1])) } }}>
          <b className="truncate text-[13px] font-semibold">{t('control.screenTitle')}</b>
          <Badge variant={paused ? 'neutral' : 'ok'} className="h-5">{t(paused ? 'control.screenPaused' : 'control.screenWorking')}</Badge>
        </button>
        <IconButton size="sm" label={t('control.openApp')} action="agentScreenFocus" data-screen-open onClick={() => void focusTarget()}><Icon name="external" size={12} /></IconButton>
        <IconButton size="sm" label={t(expanded ? 'agentScreen.collapse' : 'agentScreen.expand')} action="agentScreenExpand" on={expanded}
          onClick={() => setExpanded(v => !v)}><Icon name={expanded ? 'collapse' : 'expand'} size={12} /></IconButton>
        <IconButton size="sm" label={t('agentScreen.hide')} action="agentScreenHide" onClick={hide}><Icon name="close" size={12} /></IconButton>
      </div>
    </aside>
  </TooltipProvider>
}
