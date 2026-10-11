import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'
import type { ModelRequestRecovery } from '../gen/ModelRequestRecovery'

// MR-02 failure codes are produced by core `failure_code`; only these have a
// direct remedy screen. Transient codes (transport, rate_limited,
// provider_unavailable) and invalid_response retry instead.
const SETTINGS_CODES = new Set(['missing_credential', 'credential_store', 'authentication', 'configuration', 'request_rejected'])
const USAGE_CODES = new Set(['budget_unavailable', 'quota'])

/** Title/hint keys per item state; `turn` items name the role request. */
function copyKeys(state: string, turn: boolean, intake: boolean): [string, string] {
  if (intake && state !== 'retrying') {
    return state === 'recovered' ? ['modelRecovery.recovered', 'modelRecovery.recoveredHintIntake'] : ['modelRecovery.titleIntake', 'modelRecovery.hintIntake']
  }
  switch (state) {
    case 'recovered': return ['modelRecovery.recovered', turn ? 'modelRecovery.recoveredHintTurn' : 'modelRecovery.recoveredHint']
    case 'retrying': return [turn ? 'modelRecovery.retryingTitleTurn' : 'modelRecovery.retryingTitle', 'modelRecovery.waitingHint']
    case 'stopped': return ['modelRecovery.stopped', 'modelRecovery.stoppedHint']
    case 'needs_choice': return ['modelRecovery.choiceTitle', 'modelRecovery.choiceHint']
    case 'unknown': return ['modelRecovery.unknownTitle', 'modelRecovery.unknownHint']
    case 'superseded': return ['modelRecovery.superseded', '']
    default: return [turn ? 'modelRecovery.titleTurn' : 'modelRecovery.title', turn ? 'modelRecovery.hintTurn' : 'modelRecovery.hint']
  }
}

// MR-01: both surfaces describe the same persisted item. Only the original
// failed request can retry; generic stage recovery must never receive it.
export function ModelRecoveryStatus({ id, payload, top = false }: { id: string; payload: Record<string, unknown>; top?: boolean }) {
  const { t } = useTranslation()
  const recovery = (payload.view ?? payload) as ModelRequestRecovery
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)
  const state = recovery.state
  const latestCode = recovery.history?.at(-1)?.code
  const openSettings = (section: 'models' | 'usage') => window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: section }))
  const run = async (action: () => Promise<unknown>) => {
    if (inFlight.current) return
    if (useUiStore.getState().modalScope !== 'workbench') {
      useUiStore.getState().pushToast(t('decisions.scopeBlocked')); return
    }
    inFlight.current = true; setBusy(true)
    try {
      await action()
    } catch (error) {
      useUiStore.getState().pushToast(t('errors.actionFailed', { detail: errText(error) }), 'err')
    } finally {
      try { await useUiStore.getState().invalidate() } finally {
        inFlight.current = false; setBusy(false)
      }
    }
  }
  const label = t('modelRecovery.retry')
  const choosing = state === 'needs_choice'
  const canRetry = recovery.can_retry !== false && !['recovered', 'retrying', 'superseded', 'needs_choice'].includes(state)
  const shortcut = top && canRetry ? formatBinding(bindingFor('approve')) : ''
  const chatOffer = canRetry ? recovery.chat_model_slot : null
  const retryWithChat = () => run(() => api.retryModelRequestWithChatModel(id, recovery.project_root))
  // Ticket 06: stop this item's automatic retries (control lane; the item
  // stays for an explicit retry and nothing else revives it).
  // Not through `run`: the retry it would stop holds the in-flight guard.
  const stop = () => api.stopModelRequest(id)
    .catch(error => useUiStore.getState().pushToast(t('errors.actionFailed', { detail: errText(error) }), 'err'))
  const choose = (followNew: boolean) => run(() => api.resolveModelRequestChoice(id, followNew, recovery.project_root))
  // ADR 0051: the explicit choice has its own bindings on the top item only.
  useEffect(() => {
    if (!top || (!choosing && !chatOffer && state !== 'retrying')) return
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat || e.defaultPrevented) return
      if (state === 'retrying' && matches(e, bindingFor('modelRecoveryStop'))) {
        e.preventDefault(); void stop(); return
      }
      if (chatOffer && matches(e, bindingFor('retryWithChatModel'))) {
        e.preventDefault(); void retryWithChat(); return
      }
      if (!choosing) return
      const followNew = matches(e, bindingFor('followNewInstruction'))
      if (!followNew && !matches(e, bindingFor('continueOriginalTask'))) return
      e.preventDefault()
      void choose(followNew)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })
  const turn = recovery.purpose === 'turn'
  const target = recovery.target_role ?? t('modelRecovery.nextRole')
  const intake = recovery.purpose === 'opening_intake'
  const [title, hint] = copyKeys(state, turn, intake)
  const reason = state === 'superseded' && recovery.superseded_reason ? t(`modelRecovery.reason.${recovery.superseded_reason}`, { defaultValue: recovery.superseded_reason }) : ''
  const refusal = recovery.last_refusal && !['recovered', 'superseded'].includes(state)
    ? t('modelRecovery.lastRefusal', { reason: t(`modelRecovery.refusal.${recovery.last_refusal}`, { defaultValue: recovery.last_refusal }) }) : ''
  const deadline = state === 'retrying' && recovery.retry_deadline_ms != null ? new Date(recovery.retry_deadline_ms).toLocaleTimeString() : null
  return <div data-model-recovery={id} data-state={state} aria-busy={busy}>
    <div style={{fontWeight: 510, color: state === 'recovered' ? 'var(--ok)' : 'var(--accent)'}}>{t(title)}</div>
    <div className="dim3" style={{fontSize: 12, margin: '4px 0 8px'}}>{reason || (hint && t(hint, { target, instruction: recovery.held_instruction ?? '' }))}</div>
    {refusal && <div className="dim3" data-refusal style={{fontSize: 12, marginBottom: 8}}>{refusal}</div>}
    {deadline && <div className="dim3" data-retry-progress style={{fontSize: 12, marginBottom: 8}}>{t('modelRecovery.attempt', { count: (recovery.history?.length ?? 0) + 1, time: deadline })}</div>}
    {recovery.unfinished && recovery.partial_text && <details className="dim3" style={{fontSize: 11, marginBottom: 8}}><summary>{t('modelRecovery.unfinished')}</summary><div>{t('modelRecovery.partialHint')}</div><div className="mono" style={{whiteSpace:'pre-wrap', marginTop:4}}>{recovery.partial_text}</div></details>}
    <details className="dim3" style={{fontSize: 11, marginBottom: 8}}>
      <summary>{t('modelRecovery.details')}</summary>
      {recovery.instruction && <div style={{whiteSpace:'pre-wrap', marginBottom: 4}}>{t('modelRecovery.instruction', { text: recovery.instruction })}</div>}
      {(recovery.history ?? []).map((failure, index) => <div key={index} className="mono">
        {failure.model ?? t('modelRecovery.modelSlot', { slot: failure.model_slot })} · {failure.code} · {failure.request_id ?? t('modelRecovery.notSent')}
        {failure.http_status != null && ` · HTTP ${failure.http_status}`} · {failure.message}
      </div>)}
    </details>
    {state === 'retrying' && <button type="button" className="btn" data-stop-retry
      title={[t('modelRecovery.stopHint'), top ? formatBinding(bindingFor('modelRecoveryStop')) : ''].filter(Boolean).join(' · ')}
      onClick={() => { void stop() }}>{t('modelRecovery.stop')}</button>}
    {canRetry && <button type="button" className="btn primary" disabled={busy}
      title={[label,shortcut].filter(Boolean).join(' · ')} onClick={() => { void run(() => intake ? api.runOpeningIntake() : api.retryModelRequest(id, recovery.project_root)) }}>
      {busy ? t('modelRecovery.retrying') : label}{shortcut && ` ${shortcut}`}
    </button>}
    {chatOffer && <button type="button" className="btn" disabled={busy} data-chat-model
      title={[t('modelRecovery.chatModelHint'), top ? formatBinding(bindingFor('retryWithChatModel')) : ''].filter(Boolean).join(' · ')}
      onClick={() => { void retryWithChat() }}>{t('modelRecovery.useChatModel', { slot: chatOffer })}</button>}
    {choosing && <>
      <button type="button" className="btn primary" disabled={busy} title={[t('modelRecovery.continueOriginal'), formatBinding(bindingFor('continueOriginalTask'))].join(' · ')}
        onClick={() => { void choose(false) }}>{t('modelRecovery.continueOriginal')}</button>
      <button type="button" className="btn" disabled={busy} title={[t('modelRecovery.followNew'), formatBinding(bindingFor('followNewInstruction'))].join(' · ')}
        onClick={() => { void choose(true) }}>{t('modelRecovery.followNew')}</button>
    </>}
    {canRetry && latestCode && SETTINGS_CODES.has(latestCode) && <button type="button" className="btn" title={t('modelRecovery.openModels')} onClick={() => openSettings('models')}>{t('modelRecovery.openModels')}</button>}
    {canRetry && latestCode && USAGE_CODES.has(latestCode) && <button type="button" className="btn" title={t('modelRecovery.openUsage')} onClick={() => openSettings('usage')}>{t('modelRecovery.openUsage')}</button>}
  </div>
}
