import { blockBackgroundDecision } from '../modalDecisionGuard'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type PendingQuestion } from '../api'
import type { DesignDirection } from '../gen/DesignDirection'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'
import { DecisionButton, DecisionShell } from './DecisionShell'
import { buttonVariants } from './ui/button'
import './DesignChoices.css'

// 票 04（components D12 推荐卡，decisions#design）：头（palette 图标块 + 标题 + 副行）+ 方案三列（radio，选中 accent 内环
// + 「已选」chip）+ 底（左信号文字 + 「放大对比」default · 「用这个方向」primary，未选时禁用）。
// Owner Q5 不变：只认显式选定的那一版方案；通用批准键不替选（decisions.ts toastOnly）。
export function DesignChoices({ q }: { q: PendingQuestion }) {
  const { t } = useTranslation()
  const [direction, setDirection] = useState<DesignDirection | null>(null)
  const [guidance, setGuidance] = useState('')
  const [picked, setPicked] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const saving = useRef(false)
  const [preview, setPreview] = useState<{ page: string; data_url: string } | null>(null)
  const closePreview = useCallback(() => setPreview(null), [])
  const refresh = useUiStore((s) => s.refresh)
  // 截图比对修正（2026-10-14，decisions#design 副行「UI · 3 个方案」）：副行显示角色名而非 agent id，同 PendingCards useWho
  const role = useUiStore((s) => s.team.find((m) => m.id === q.agent_id)?.role) ?? String(q.agent_id ?? '')
  useEffect(() => {
    let cancelled = false
    setPicked(null)
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
  const options = current && direction ? direction.options : []
  const selected = options.find((o) => o.id === picked) ?? null
  const compare = (selected ?? options[0])?.mockups[0]
  return <DecisionShell qid={q.id} cat="info" icon="palette" title={t('design.title')}
    sub={t('design.sub', { role, count: options.length })}
    note={current && direction ? t('design.revision', { revision: direction.revision }) : undefined}
    actions={current && direction && options.length > 0 && <>
      <DecisionButton disabled={!compare} onClick={() => compare && setPreview(compare)}
        title={[t('design.preview'), formatBinding(bindingFor('previewDesignMockup'))].filter(Boolean).join(' · ')}>{t('design.compare')}</DecisionButton>
      <DecisionButton variant="primary" busy={busy} disabled={!selected} title={title('design.choose')}
        onClick={() => { if (selected) void choose(selected.id) }}>{t('design.choose')}</DecisionButton>
    </>}>
    <section className="design-choices" aria-label={t('design.title')} aria-busy={busy}>
      <p>{t('design.intro')}</p>
      {error && <p role="alert" className="design-error">{error}</p>}
      {!direction ? <p role="status">{t('design.loading')}</p> : !current ? <p role="status">{t('design.stale')}</p> : <>
        {options.length === 0 && <p>{t('design.empty')}</p>}
        <div className="design-options" role="radiogroup" aria-label={t('design.title')}>{options.map((option) => {
          const on = option.id === picked
          return <article key={option.id} className="design-option" data-selected={on || undefined}>
            {option.mockups.slice(0, 1).map((mockup) => <figure key={mockup.digest}>
              <button type="button" className="design-mockup-trigger" aria-label={t('design.previewPage', { page: mockup.page })}
                title={[t('design.preview'), formatBinding(bindingFor('previewDesignMockup'))].filter(Boolean).join(' · ')}
                onClick={event => { event.currentTarget.focus(); setPreview(mockup) }}
                onKeyDown={event => { if (matches(event.nativeEvent, bindingFor('previewDesignMockup'))) { event.preventDefault(); event.currentTarget.click() } }}>
                <img src={mockup.data_url} alt={mockup.page} loading="lazy" decoding="async" />
              </button>
              {on && <span className="chip info design-picked">{t('design.picked')}</span>}
            </figure>)}
            <label className="design-pick">
              <input type="radio" name={`design-${q.id}`} checked={on} disabled={busy} onChange={() => setPicked(option.id)} />
              <b>{option.title}</b>
            </label>
            <p>{option.description}</p>
            <p className="design-meta">{[option.layout, option.typography, option.palette].filter(Boolean).join(' · ')}</p>
          </article>
        })}</div>
        <details className="design-existing">
          <summary>{t('design.existingLabel')}</summary>
          <textarea aria-label={t('design.existingLabel')} value={guidance} maxLength={4000} rows={3} disabled={busy} placeholder={t('design.existingPlaceholder')} onChange={(event) => setGuidance(event.target.value)} />
          <DecisionButton disabled={busy || !guidance.trim()} title={title('design.useExisting')} onClick={() => void choose(null)}>{t('design.useExisting')}</DecisionButton>
        </details>
      </>}
    </section>
    {preview && <DesignMockupPreview key={q.id} image={preview} onClose={closePreview} />}
  </DecisionShell>
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
    <header><h3>{image.page}</h3><button ref={closeButton} type="button" className={buttonVariants({ size: 'sm' })} onClick={onClose}
      title={[t('design.closePreview'), formatBinding(bindingFor('closeDesignPreview'))].filter(Boolean).join(' · ')}>{t('design.closePreview')}</button></header>
    <div className="design-preview-image"><img src={image.data_url} alt={image.page} decoding="async" /></div>
  </dialog>
}
