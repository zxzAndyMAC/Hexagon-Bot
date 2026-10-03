import { blockBackgroundDecision } from '../modalDecisionGuard'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type PendingQuestion } from '../api'
import type { DesignDirection } from '../gen/DesignDirection'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'
import './DesignChoices.css'

export function DesignChoices({ q }: { q: PendingQuestion }) {
  const { t } = useTranslation()
  const [direction, setDirection] = useState<DesignDirection | null>(null)
  const [guidance, setGuidance] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const saving = useRef(false)
  const [preview, setPreview] = useState<{ page: string; data_url: string } | null>(null)
  const closePreview = useCallback(() => setPreview(null), [])
  const refresh = useUiStore((s) => s.refresh)
  useEffect(() => {
    let cancelled = false
    api.designDirection().then((value) => { if (!cancelled) setDirection(value) })
      .catch((e: unknown) => { if (!cancelled) setError(errText(e)) })
    return () => { cancelled = true }
  }, [q.id])
  const current = direction?.question_id === q.id && direction.revision === Number(q.payload.revision)
  const choose = async (id: string | null) => {
    if (!current || saving.current || !direction) return
    saving.current = true
    setBusy(true)
    setError('')
    try {
      await api.chooseDesignDirection(q.id, direction.revision, id ?? undefined, id === null ? guidance.trim() : undefined)
      await refresh()
    } catch (e) { setError(errText(e)) }
    finally { saving.current = false; setBusy(false) }
  }
  const shortcut = formatBinding(bindingFor('chooseDesignDirection'))
  const title = (key: string) => [t(key), shortcut].filter(Boolean).join(' · ')
  return <section className="design-choices" aria-label={t('design.title')} aria-busy={busy}>
    <h3>{t('design.title')}</h3>
    <p className="dim">{t('design.intro')}</p>
    {error && <p role="alert" className="design-error">{error}</p>}
    {!direction ? <p role="status">{t('design.loading')}</p> : !current ? <p role="status">{t('design.stale')}</p> : <>
      <p className="dim3">{t('design.revision', { revision: direction.revision })}</p>
      {direction.options.length === 0 && <p className="dim">{t('design.empty')}</p>}
      <div className="design-options">{direction.options.map((option) => <article key={option.id} className="design-option">
        <h4>{option.title}</h4>
        {option.mockups.map((mockup) => <figure key={mockup.digest}>
          <button type="button" className="design-mockup-trigger" aria-label={t('design.previewPage', { page: mockup.page })}
            title={[t('design.preview'), formatBinding(bindingFor('previewDesignMockup'))].filter(Boolean).join(' · ')}
            onClick={event => { event.currentTarget.focus(); setPreview(mockup) }}
            onKeyDown={event => { if (matches(event.nativeEvent, bindingFor('previewDesignMockup'))) { event.preventDefault(); event.currentTarget.click() } }}>
            <img src={mockup.data_url} alt={mockup.page} loading="lazy" decoding="async" />
          </button>
          <figcaption>{mockup.page}</figcaption>
        </figure>)}
        <p>{option.description}</p>
        <dl>{(['layout', 'typography', 'palette'] as const).map((field) => <div key={field}>
          <dt>{t(`design.${field}`)}</dt><dd>{option[field]}</dd>
        </div>)}</dl>
        <button className="btn primary" disabled={busy} title={title('design.choose')} onClick={() => void choose(option.id)}>{t('design.choose')}</button>
      </article>)}</div>
      <label className="design-existing">{t('design.existingLabel')}
        <textarea value={guidance} maxLength={4000} rows={3} disabled={busy} placeholder={t('design.existingPlaceholder')} onChange={(event) => setGuidance(event.target.value)} />
      </label>
      <button className="btn" disabled={busy || !guidance.trim()} title={title('design.useExisting')} onClick={() => void choose(null)}>{t('design.useExisting')}</button>
    </>}
    {preview && <DesignMockupPreview key={q.id} image={preview} onClose={closePreview} />}
  </section>
}


function DesignMockupPreview({ image, onClose }: { image: { page: string; data_url: string }; onClose: () => void }) {
  const { t } = useTranslation()
  const dialog = useRef<HTMLDialogElement>(null)
  const closeButton = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    const previous = document.activeElement
    const modal = dialog.current
    if (modal?.showModal) modal.showModal()
    else modal?.setAttribute('open', '')
    closeButton.current?.focus()
    // Owner 2026-10-01: a native modal blocks pointer input, but global
    // keyboard handlers still run. Do not let inspecting a mockup decide a card.
    const keys = (event: KeyboardEvent) => {
      if (event.key === 'Escape' || matches(event, bindingFor('closeDesignPreview'))) {
        event.preventDefault(); event.stopImmediatePropagation(); onClose(); return
      }
      if (blockBackgroundDecision(event)) {
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
  return <dialog ref={dialog} className="design-preview" aria-modal="true" aria-label={t('design.previewPage', { page: image.page })}
    onCancel={event => { event.preventDefault(); onClose() }}>
    <header><h3>{image.page}</h3><button ref={closeButton} type="button" className="btn" onClick={onClose}
      title={[t('design.closePreview'), formatBinding(bindingFor('closeDesignPreview'))].filter(Boolean).join(' · ')}>{t('design.closePreview')}</button></header>
    <div className="design-preview-image"><img src={image.data_url} alt={image.page} decoding="async" /></div>
  </dialog>
}
