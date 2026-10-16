import { blockBackgroundDecision } from '../modalDecisionGuard'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'
import { Button } from '@/components/ui/button'
import { ShortcutKbd } from '@/components/ui/kbd'
import { sameImageSize } from '../computerTargetRing'

// variant 'card'：电脑动作确认卡（components D8，control#action；ui-redesign gap D6）左栏 200 宽缩略，
// 不带说明行（卡上 kv 已写应用 / 动作），放大仍走同一模态与同一受限读取 IPC。
// ring（gap D8）：动作点在缩略框里的比例位置（computerTargetRing.ringPoint 已换算 object-cover 裁切）；
// 只有加载出的像素尺寸与证据记录的 image_width/height 一致才画——不是同一张图就不画，绝不画错位置。
type Props = { output: Record<string, unknown>; projectId?: string; capturedAt?: string; variant?: 'card'; ring?: { left: number; top: number } | null }
type ScreenshotResult = { screenshot_path?: string; title?: string; bundle_id?: string; image_width?: number; image_height?: number }

// 截图证据（components D19「截图证据」，control#screen；票 ui-redesign 14 换皮）：工具行下 120 宽缩略
// （16:10、--r-sm、card 阴影）+ 说明「截图证据 · 应用 · 时间」「点击放大，Esc 关闭」；放大是居中模态，
// 放大时裁决快捷键被拦（blockBackgroundDecision）——「放大时不触发裁决」。
/** Owner 2026-10-02, extension 11: pixels are local, ephemeral evidence.
 * Never persist the image in the trace or infer success from dispatched input. */
export function DesktopToolDetails(props: Props) {
  const result = props.output.result as ScreenshotResult | undefined
  return <DesktopScreenshotDetails key={`${props.projectId ?? ''}:${result?.screenshot_path ?? ''}`} {...props} />
}

function DesktopScreenshotDetails({ output, capturedAt, variant, ring }: Props) {
  const { t } = useTranslation()
  const [image, setImage] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState(false)
  const [preview, setPreview] = useState(false)
  const [sizeOk, setSizeOk] = useState(false)
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
    capturedAt ? new Date(capturedAt).toLocaleString() : variant === 'card' ? null : t('computer.screenshotUnknownTime')].filter(Boolean).join(' · ')
  const card = variant === 'card'
  const title = [t('computer.viewScreenshot'), formatBinding(bindingFor('viewDesktopScreenshot'))].filter(Boolean).join(' · ')
  return <div className={card ? 'desktop-tool-details' : 'desktop-tool-details mt-1'}>
    {output.requires_observation === true && <p role="status" className="text-[12px] text-orange">{t('computer.needsObservation')}</p>}
    {name && <figure className={card ? 'm-0' : 'mt-1 flex items-center gap-2'}>
      <button ref={trigger} className={`desktop-screenshot-thumbnail hx-focus grid aspect-[16/10] ${card ? 'relative w-full' : 'w-[120px]'} rounded-[var(--r-sm)] flex-none cursor-zoom-in place-items-center overflow-hidden bg-inset p-0 text-[11px] text-ink-3 shadow-[var(--shadow-card)] disabled:cursor-default disabled:opacity-45`}
        type="button" disabled={failed} aria-busy={busy}
        aria-label={t('computer.viewScreenshot')} title={title}
        onClick={event => { event.currentTarget.focus(); setPreview(true); if (!image) void load() }}
        onKeyDown={event => { if (matches(event.nativeEvent, bindingFor('viewDesktopScreenshot'))) { event.preventDefault(); event.currentTarget.click() } }}>
        {image ? <img src={image} alt={result?.title || t('computer.screenshotAlt')} decoding="async" className="size-full object-cover"
          onLoad={event => setSizeOk(sameImageSize({ width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight }, result?.image_width, result?.image_height))} /> : <span>{t(busy ? 'computer.screenshotLoading' : 'computer.viewScreenshot')}</span>}
        {card && ring && image && sizeOk && <span data-testid="computer-target-ring" aria-hidden="true"
          className="computer-target-ring pointer-events-none absolute size-[22px] -translate-x-1/2 -translate-y-1/2 rounded-full"
          style={{ left: `${ring.left * 100}%`, top: `${ring.top * 100}%` }} />}
      </button>
      {!card && <figcaption className="min-w-0 text-[12px] text-ink-3 [overflow-wrap:anywhere]">{t('control.evidence', { label })}<br />{t('control.evidenceHint')}</figcaption>}
    </figure>}
    {failed && <p role="status" className="mt-1 text-[12px] text-ink-3">{t('computer.screenshotUnavailable')}</p>}
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
  return <dialog ref={dialog} aria-modal="true" aria-label={t('computer.viewScreenshot')}
    className="desktop-screenshot-preview m-auto max-h-[94vh] w-[min(1400px,94vw)] max-w-[94vw] rounded-[var(--r-panel)] bg-surface p-3 text-ink shadow-[var(--shadow-overlay)] backdrop:bg-scrim"
    onCancel={event => { event.preventDefault(); onClose() }}>
    <header className="mb-2.5 flex items-center justify-between gap-4 text-[13px] text-ink-2 [overflow-wrap:anywhere]">
      <span>{label}</span>
      <Button ref={closeButton} size="sm" className="flex-none" onClick={onClose}
        title={[t('computer.closeScreenshot'), formatBinding(bindingFor('closeDesktopScreenshot'))].filter(Boolean).join(' · ')}>
        {t('computer.closeScreenshot')}<ShortcutKbd binding={bindingFor('closeDesktopScreenshot') || 'Escape'} />
      </Button>
    </header>
    <img src={image} alt={label} decoding="async" className="block max-h-[calc(90vh-80px)] w-full rounded-[var(--r-sm)] object-contain" />
  </dialog>
}
