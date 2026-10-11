import { blockBackgroundDecision } from '../modalDecisionGuard'
import { useEffect, useId, useLayoutEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { bindingFor, formatBinding, matches } from '../keymap'
import { Icon } from './Icon'

/** 2026-10-11 UI alignment: turning delegation on always passes this one
 * explanation (same form as the broad-access confirmation): centred modal,
 * Cancel focused, no default confirm key, background decisions blocked. */
export function ManagedConfirmation({ pending, error, onCancel, onConfirm }: {
  pending: boolean; error: boolean; onCancel: () => void; onConfirm: () => void
}) {
  const { t } = useTranslation()
  const id = useId()
  const dialog = useRef<HTMLDialogElement>(null)
  const cancel = useRef<HTMLButtonElement>(null)
  const current = useRef({ pending, onCancel, onConfirm })
  useLayoutEffect(() => { current.current = { pending, onCancel, onConfirm } }, [pending, onCancel, onConfirm])
  useEffect(() => {
    if (dialog.current?.showModal) dialog.current.showModal()
    else dialog.current?.setAttribute('open', '')
    cancel.current?.focus()
    const block = (event: KeyboardEvent) => {
      if (matches(event, bindingFor('confirmManaged'))) {
        event.preventDefault(); event.stopImmediatePropagation()
        if (!current.current.pending) current.current.onConfirm()
        return
      }
      blockBackgroundDecision(event)
    }
    window.addEventListener('keydown', block, true)
    return () => window.removeEventListener('keydown', block, true)
  }, [])
  const can = t('delegation.can', { returnObjects: true }) as string[]
  const cannot = t('delegation.cannot', { returnObjects: true }) as string[]
  return <dialog ref={dialog} className="approval-mode-confirm managed-confirm" role="alertdialog" aria-modal="true"
    aria-labelledby={`${id}-title`} aria-describedby={`${id}-body`} aria-busy={pending}
    onCancel={event => { event.preventDefault(); if (!pending) onCancel() }}>
    <h2 id={`${id}-title`}><Icon name="agent" />{t('delegation.confirmTitle')}</h2>
    <div id={`${id}-body`}>
      <p>{t('delegation.confirmBody')}</p>
      <div className="managed-confirm-columns">
        <section><h3>{t('delegation.canTitle')}</h3><ul>{can.map(line => <li key={line}>{line}</li>)}</ul></section>
        <section><h3>{t('delegation.cannotTitle')}</h3><ul>{cannot.map(line => <li key={line}>{line}</li>)}</ul></section>
      </div>
      <p>{t('delegation.limits')}</p>
    </div>
    {error && <p role="alert" className="approval-mode-error">{t('delegation.failed')}</p>}
    <div className="approval-mode-confirm-actions">
      <button ref={cancel} type="button" className="btn" disabled={pending} onClick={onCancel} title={[t('agent.cancel'), 'Esc'].join(' · ')}>{t('agent.cancel')}</button>
      <button type="button" className="btn" disabled={pending} onClick={onConfirm} title={[t('delegation.confirmAction'), formatBinding(bindingFor('confirmManaged'))].filter(Boolean).join(' · ')}>{t('delegation.confirmAction')}</button>
    </div>
  </dialog>
}
