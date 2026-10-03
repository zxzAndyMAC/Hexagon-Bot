import { useEffect, useId, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type PendingQuestion } from '../api'
import type { PermissionShapeSuggestion } from '../gen/PermissionShapeSuggestion'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'
import { Icon } from './Icon'
import './ApprovalModeSelector.css'
import './PermissionActions.css'

export function PermissionScope({ tool, shape, generalized = false }: { tool: string; shape: string; generalized?: boolean }) {
  const { t } = useTranslation()
  const target = shape.startsWith('exact:') ? shape.slice(6) : generalized ? shape.replace(/\*$/, '') : shape
  return <span className="permission-scope">{t(`projectPermission.action_${tool}`, { target })}</span>
}

/** ADR 0079: this component is keyed by card ID. A new card never inherits an
 * open menu or a broader choice; shortcuts continue to mean allow once. */
export function PermissionActions({ q }: { q: PendingQuestion }) {
  const { t } = useTranslation()
  const [proposal, setProposal] = useState<PermissionShapeSuggestion | null>(null)
  const [ready, setReady] = useState(false)
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)
  const trigger = useRef<HTMLButtonElement>(null)
  const container = useRef<HTMLDivElement>(null)
  const menuId = useId()
  const safety = Boolean(q.payload.safety_net)
  const invalidate = useUiStore((s) => s.invalidate)
  const pushToast = useUiStore((s) => s.pushToast)
  useEffect(() => {
    if (safety) return
    let active = true
    void api.permissionShapeSuggestion(q.id).then((value) => {
      if (active) { setProposal(value); setReady(true) }
    }).catch(() => { if (active) setReady(true) })
    return () => { active = false }
  }, [q.id, safety])
  useEffect(() => {
    if (!open) return
    container.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus()
    const close = (event: PointerEvent) => {
      if (event.target instanceof Node && !container.current?.contains(event.target)) setOpen(false)
    }
    document.addEventListener('pointerdown', close)
    return () => document.removeEventListener('pointerdown', close)
  }, [open])
  async function choose(choice: 'once' | 'project' | 'deny') {
    if (inFlight.current) return
    inFlight.current = true
    setBusy(true)
    try {
      if (choice === 'project') await api.allowProjectPermission(q.id)
      else await api.answerPermission(q.id, choice === 'once')
      setOpen(false)
      await invalidate()
    } catch (error) {
      pushToast(t('errors.actionFailed', { detail: errText(error) }), 'err')
      // ADR 0079: a grant can commit and consume cards before execution fails.
      await invalidate()
    } finally {
      inFlight.current = false
      setBusy(false)
    }
  }
  const input = q.payload.input as Record<string, unknown> | undefined
  return <div ref={container} onKeyDown={(event) => {
    if (proposal && !busy && matches(event.nativeEvent, bindingFor('permissionOptions'))) {
      event.preventDefault(); event.stopPropagation(); setOpen(!open)
    }
    if (event.key === 'Escape' && open) {
      event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus()
    }
  }}>
    {(safety || (ready && !proposal)) && <p className="dim3 permission-scope">{t(safety ? 'projectPermission.onceRequired' : 'projectPermission.unavailable')}</p>}
    <div className="permission-actions">
      <div className="approval-mode permission-split">
        <button type="button" className="btn primary" disabled={busy} title={formatBinding(bindingFor('approve'))}
          onClick={() => { void choose('once') }}>{t('cards.allowOnce')}</button>
        {proposal && <button ref={trigger} type="button" className="btn primary" disabled={busy}
          aria-haspopup="menu" aria-expanded={open} aria-controls={menuId}
          aria-label={t('projectPermission.options')} title={[t('projectPermission.options'), formatBinding(bindingFor('permissionOptions'))].filter(Boolean).join(' · ')}
          onClick={() => setOpen(!open)} onKeyDown={(event) => {
            if (event.key === 'ArrowDown') { event.preventDefault(); setOpen(true) }
          }}><Icon name="chevron-down" size={12} /></button>}
      <button type="button" className="btn danger" disabled={busy} title={formatBinding(bindingFor('reject'))}
        onClick={() => { void choose('deny') }}>{t('cards.deny')}</button>
        {open && proposal && <div id={menuId} role="menu" aria-label={t('projectPermission.options')}
          className="approval-mode-menu permission-menu" aria-busy={busy}
          onKeyDown={(event) => {
            const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
            const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
            const next = event.key === 'ArrowDown' ? (index + 1) % buttons.length
              : event.key === 'ArrowUp' ? (index - 1 + buttons.length) % buttons.length
              : event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : -1
            if (next >= 0) { event.preventDefault(); buttons[next]?.focus() }
            if (event.key === 'Tab') { setOpen(false); trigger.current?.focus() }
          }}>
          <button type="button" role="menuitem" className="approval-mode-option" disabled={busy}
            title={formatBinding(bindingFor('approve'))} onClick={() => { void choose('once') }}>
            <span className="approval-mode-option-copy"><strong>{t('cards.allowOnce')}</strong><span>{t('projectPermission.onceHint')}</span></span>
          </button>
          <button type="button" role="menuitem" className="approval-mode-option" disabled={busy}
            onClick={() => { void choose('project') }}>
            <span className="approval-mode-option-copy"><strong>{t('projectPermission.allow')}</strong>
              <PermissionScope tool={proposal.tool} shape={proposal.shape} generalized={proposal.generalized} />
              <span>{t('projectPermission.shared')}</span>
              {proposal.tool === 'bash' && <span>{t(input?.net ? 'perms.networkAllowed' : 'perms.networkDisabled')} · {t(input?.background ? 'perms.backgroundAllowed' : 'perms.foregroundOnly')} · {input?.session ? t('perms.namedSession', { name: String(input.session) }) : t('perms.noNamedSession')}</span>}
            </span>
          </button>
        </div>}
      </div>

    </div>
  </div>
}
