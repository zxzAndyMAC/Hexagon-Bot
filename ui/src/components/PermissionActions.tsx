import { useEffect, useId, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type PendingQuestion } from '../api'
import type { PermissionShapeSuggestion } from '../gen/PermissionShapeSuggestion'
import { bindingFor, formatBinding, matches } from '../keymap'
import { settleDecision } from '../decisions'
import { Icon } from './Icon'
import { DecisionButton } from './DecisionShell'

export function PermissionScope({ tool, shape, generalized = false }: { tool: string; shape: string; generalized?: boolean }) {
  const { t } = useTranslation()
  const target = shape.startsWith('exact:') ? shape.slice(6) : generalized ? shape.replace(/\*$/, '') : shape
  return <span className="permission-scope">{t(`projectPermission.action_${tool}`, { target })}</span>
}

/** ADR 0079: this component is keyed by card ID. A new card never inherits an
 * open menu or a broader choice; shortcuts continue to mean allow once.
 * 票 04（components D6 底栏、decisions#dock）：坞底栏的左说明 + 右按钮组——拒绝（quiet）·
 * 「允许一次 ▾」组合按钮（A1 split）。▾ 只在宿主给出可靠范围时出现；安全网动作只给单次与拒绝。
 * 范围菜单向上弹在坞上方（A16 弹层形），键盘行为沿用 ADR 0079 的自绘菜单。 */
export function PermissionActions({ q, top = false }: { q: PendingQuestion; top?: boolean }) {
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
    setOpen(false)
    // ADR 0079: a grant can commit and consume cards before execution fails —
    // settleDecision always re-reads (invalidate) after success or failure.
    await settleDecision(q.id, () => choice === 'project' ? api.allowProjectPermission(q.id) : api.answerPermission(q.id, choice === 'once'))
    inFlight.current = false
    setBusy(false)
  }
  const input = q.payload.input as Record<string, unknown> | undefined
  const note = safety ? t('projectPermission.onceRequired') : ready && !proposal ? t('projectPermission.unavailable') : t('dock.onceNote')
  return <div ref={container} className="dock-perm" onKeyDown={(event) => {
    if (proposal && !busy && matches(event.nativeEvent, bindingFor('permissionOptions'))) {
      event.preventDefault(); event.stopPropagation(); setOpen(!open)
    }
    if (event.key === 'Escape' && open) {
      event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus()
    }
  }}>
    <span className="dock-note permission-scope">{note}</span>
    <DecisionButton variant="quiet" busy={busy} action="reject" showKey={top} onClick={() => { void choose('deny') }}>{t('cards.deny')}</DecisionButton>
    <span className="dock-split" data-split={proposal ? '' : undefined}>
      <DecisionButton variant="primary" busy={busy} action="approve" showKey={top} onClick={() => { void choose('once') }}>{t('cards.allowOnce')}</DecisionButton>
      {proposal && <button ref={trigger} type="button" data-slot="button" data-variant="primary" className="dock-caret" disabled={busy}
        aria-haspopup="menu" aria-expanded={open} aria-controls={menuId}
        aria-label={t('projectPermission.options')} title={[t('projectPermission.options'), formatBinding(bindingFor('permissionOptions'))].filter(Boolean).join(' · ')}
        onClick={() => setOpen(!open)} onKeyDown={(event) => {
          if (event.key === 'ArrowDown') { event.preventDefault(); setOpen(true) }
        }}><Icon name="chevron-down" size={12} /></button>}
    </span>
    {open && proposal && <div id={menuId} role="menu" aria-label={t('projectPermission.options')}
      className="dock-menu" aria-busy={busy}
      onKeyDown={(event) => {
        const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
        const next = event.key === 'ArrowDown' ? (index + 1) % buttons.length
          : event.key === 'ArrowUp' ? (index - 1 + buttons.length) % buttons.length
          : event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : -1
        if (next >= 0) { event.preventDefault(); buttons[next]?.focus() }
        if (event.key === 'Tab') { setOpen(false); trigger.current?.focus() }
      }}>
      <button type="button" role="menuitem" className="dock-mi" disabled={busy}
        title={formatBinding(bindingFor('approve'))} onClick={() => { void choose('once') }}>
        <Icon name="check" size={14} />
        <span><strong>{t('cards.allowOnce')}</strong><span>{t('projectPermission.onceHint')}</span></span>
      </button>
      <button type="button" role="menuitem" className="dock-mi" disabled={busy}
        onClick={() => { void choose('project') }}>
        <span className="dock-mi-pad" />
        <span><strong>{t('projectPermission.allow')}</strong>
          <PermissionScope tool={proposal.tool} shape={proposal.shape} generalized={proposal.generalized} />
          <span>{t('projectPermission.shared')}</span>
          {proposal.tool === 'bash' && <span>{t(input?.net ? 'perms.networkAllowed' : 'perms.networkDisabled')} · {t(input?.background ? 'perms.backgroundAllowed' : 'perms.foregroundOnly')} · {input?.session ? t('perms.namedSession', { name: String(input.session) }) : t('perms.noNamedSession')}</span>}
        </span>
      </button>
    </div>}
  </div>
}
