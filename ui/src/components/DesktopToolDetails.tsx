import { blockBackgroundDecision } from '../modalDecisionGuard'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'
import './DesktopToolDetails.css'

type Props = { output: Record<string, unknown>; projectId?: string; capturedAt?: string }
type ScreenshotResult = { screenshot_path?: string; title?: string; bundle_id?: string }

/** Owner 2026-10-02, extension 11: pixels are local, ephemeral evidence.
 * Never persist the image in the trace or infer success from dispatched input. */
export function DesktopToolDetails(props: Props) {
  const result = props.output.result as ScreenshotResult | undefined
  return <DesktopScreenshotDetails key={`${props.projectId ?? ''}:${result?.screenshot_path ?? ''}`} {...props} />
}

function DesktopScreenshotDetails({ output, capturedAt }: Props) {
  const { t } = useTranslation()
  const [image, setImage] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)
  const [preview, setPreview] = useState(false)
  const trigger = useRef<HTMLButtonElement>(null)
  const lifecycle = useRef({ generation: 0, requested: false })
  const result = output.result as ScreenshotResult | undefined
  const name = result?.screenshot_path?.split(/[\\/]/).at(-1)
  // PROJECT_ID is the same p1 in every workspace (issue 11 review). Bind the
  // basename read to the root from its persisted evidence path; the host only
  // compares this root and never opens a client-supplied absolute path.
  const marker = '/.hexagon/computer-use/screenshots/'
  const path = result?.screenshot_path ?? ''
  const boundary = path.lastIndexOf(marker)
  const expectedRoot = boundary > 0 ? path.slice(0, boundary) : null
  const close = useCallback(() => setPreview(false), [])
  const load = useCallback(async () => {
    if (!name || lifecycle.current.requested) return
    if (!expectedRoot) { setFailed(true); return }
    lifecycle.current.requested = true
    const current = lifecycle.current.generation
    setBusy(true)
    try {
      const screenshot = await api.desktopScreenshot(name, expectedRoot)
      if (current === lifecycle.current.generation) setImage(screenshot.data_url)
    } catch { if (current === lifecycle.current.generation) setFailed(true) }
    finally { if (current === lifecycle.current.generation) setBusy(false) }
  }, [name, expectedRoot])
  useEffect(() => {
    const state = lifecycle.current
    const cleared = () => {
      lifecycle.current.generation++
      lifecycle.current.requested = true
      setImage(null); setFailed(true); setBusy(false); setPreview(false)
    }
    window.addEventListener('hexagon:screenshots-cleared', cleared)
    // Clearing/unmounting must invalidate the read itself, not just the current
    // pixels: a delayed IPC used to restore a cleared screenshot (issue 11).
    return () => { state.generation++; state.requested = false; window.removeEventListener('hexagon:screenshots-cleared', cleared) }
  }, [])
  useEffect(() => {
    const node = trigger.current
    if (!node || typeof IntersectionObserver === 'undefined') return
    const observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) { void load(); observer.disconnect() }
    })
    observer.observe(node)
    return () => observer.disconnect()
  }, [load])
  const label = [result?.bundle_id || t('computer.screenshotUnknownApp'), result?.title,
    capturedAt ? new Date(capturedAt).toLocaleString() : t('computer.screenshotUnknownTime')].filter(Boolean).join(' · ')
  const title = [t('computer.viewScreenshot'), formatBinding(bindingFor('viewDesktopScreenshot'))].filter(Boolean).join(' · ')
  return <div className="desktop-tool-details">
    {output.requires_observation === true && <p role="status">{t('computer.needsObservation')}</p>}
    {name && <figure>
      <button ref={trigger} className="desktop-screenshot-thumbnail" type="button" disabled={failed} aria-busy={busy}
        aria-label={t('computer.viewScreenshot')} title={title}
        onClick={event => { event.currentTarget.focus(); setPreview(true); if (!image) void load() }}
        onKeyDown={event => { if (matches(event.nativeEvent, bindingFor('viewDesktopScreenshot'))) { event.preventDefault(); event.currentTarget.click() } }}>
        {image ? <img src={image} alt={result?.title || t('computer.screenshotAlt')} decoding="async" /> : <span>{t(busy ? 'computer.screenshotLoading' : 'computer.viewScreenshot')}</span>}
      </button>
      <figcaption>{label}</figcaption>
    </figure>}
    {failed && <p role="status">{t('computer.screenshotUnavailable')}</p>}
    {preview && image && <ScreenshotPreview image={image} label={label} onClose={close} />}
  </div>
}

function ScreenshotPreview({ image, label, onClose }: { image: string; label: string; onClose: () => void }) {
  const { t } = useTranslation()
  const dialog = useRef<HTMLDialogElement>(null)
  const closeButton = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    const previous = document.activeElement
    const modal = dialog.current
    if (modal?.showModal) modal.showModal()
    else modal?.setAttribute('open', '')
    closeButton.current?.focus()
    // Native modal pointer isolation does not stop window-level decisions.
    // Keep the emergency computer-pause shortcut available (owner issue 11).
    const keys = (event: KeyboardEvent) => {
      if (event.key === 'Escape' || matches(event, bindingFor('closeDesktopScreenshot'))) {
        event.preventDefault(); event.stopImmediatePropagation(); onClose()
      } else if (blockBackgroundDecision(event)) {
        useUiStore.getState().pushToast(t('decisions.scopeBlocked'))
      }
    }
    window.addEventListener('keydown', keys, true)
    return () => {
      window.removeEventListener('keydown', keys, true)
      if (modal?.open && modal.close) modal.close()
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus()
    }
  }, [onClose, t])
  return <dialog ref={dialog} className="desktop-screenshot-preview" aria-modal="true" aria-label={t('computer.viewScreenshot')}
    onCancel={event => { event.preventDefault(); onClose() }}>
    <header><span>{label}</span><button ref={closeButton} className="btn" type="button" onClick={onClose}
      title={[t('computer.closeScreenshot'), formatBinding(bindingFor('closeDesktopScreenshot'))].filter(Boolean).join(' · ')}>{t('computer.closeScreenshot')}</button></header>
    <img src={image} alt={label} decoding="async" />
  </dialog>
}
