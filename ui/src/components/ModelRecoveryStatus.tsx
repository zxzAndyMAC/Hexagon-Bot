import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'
import type { ModelRequestRecovery } from '../gen/ModelRequestRecovery'

// MR-02 failure codes are produced by core `failure_code`; only these have a
// direct remedy screen. Transient codes (transport, rate_limited,
// provider_unavailable) and invalid_response retry instead.
const SETTINGS_CODES = new Set(['missing_credential', 'credential_store', 'authentication', 'configuration', 'request_rejected'])
const USAGE_CODES = new Set(['budget_unavailable', 'quota'])

// MR-01: both surfaces describe the same persisted item. Only the original
// failed closed choice can retry; generic stage recovery must never receive it.
export function ModelRecoveryStatus({ id, payload, top = false }: { id: string; payload: Record<string, unknown>; top?: boolean }) {
  const { t } = useTranslation()
  const recovery = (payload.view ?? payload) as ModelRequestRecovery
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)
  const recovered = recovery.state === 'recovered'
  const retrying = recovery.state === 'retrying'
  const stopped = recovery.state === 'stopped'
  const latestCode = recovery.history?.at(-1)?.code
  const openSettings = (section: 'models' | 'usage') => window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: section }))
  const retry = async () => {
    if (inFlight.current) return
    if (useUiStore.getState().modalScope !== 'workbench') {
      useUiStore.getState().pushToast(t('decisions.scopeBlocked')); return
    }
    inFlight.current = true; setBusy(true)
    try {
      await api.retryModelRequest(id, recovery.project_root)
    } catch (error) {
      useUiStore.getState().pushToast(t('errors.actionFailed', { detail: errText(error) }), 'err')
    } finally {
      try { await useUiStore.getState().invalidate() } finally {
        inFlight.current = false; setBusy(false)
      }
    }
  }
  const label = t('modelRecovery.retry')
  const canRetry = recovery.can_retry !== false && !recovered && !retrying
  const shortcut = top && canRetry ? formatBinding(bindingFor('approve')) : ''
  const turn = recovery.purpose === 'turn'
  const target = recovery.target_role ?? t('modelRecovery.nextRole')
  const title = recovered ? 'modelRecovery.recovered' : stopped ? 'modelRecovery.stopped'
    : turn ? (retrying ? 'modelRecovery.retryingTitleTurn' : 'modelRecovery.titleTurn')
    : retrying ? 'modelRecovery.retryingTitle' : 'modelRecovery.title'
  const hint = recovered ? (turn ? 'modelRecovery.recoveredHintTurn' : 'modelRecovery.recoveredHint')
    : retrying ? 'modelRecovery.waitingHint' : stopped ? 'modelRecovery.stoppedHint'
    : turn ? 'modelRecovery.hintTurn' : 'modelRecovery.hint'
  const deadline = retrying && recovery.retry_deadline_ms != null ? new Date(recovery.retry_deadline_ms).toLocaleTimeString() : null
  return <div data-model-recovery={id} aria-busy={busy}>
    <div style={{fontWeight: 510, color: recovered ? 'var(--ok)' : 'var(--accent)'}}>{t(title)}</div>
    <div className="dim3" style={{fontSize: 12, margin: '4px 0 8px'}}>{t(hint, { target })}</div>
    {deadline && <div className="dim3" data-retry-progress style={{fontSize: 12, marginBottom: 8}}>{t('modelRecovery.attempt', { count: (recovery.history?.length ?? 0) + 1, time: deadline })}</div>}
    {recovery.unfinished && recovery.partial_text && <details className="dim3" style={{fontSize: 11, marginBottom: 8}}><summary>{t('modelRecovery.unfinished')}</summary><div>{t('modelRecovery.partialHint')}</div><div className="mono" style={{whiteSpace:'pre-wrap', marginTop:4}}>{recovery.partial_text}</div></details>}
    <details className="dim3" style={{fontSize: 11, marginBottom: 8}}>
      <summary>{t('modelRecovery.details')}</summary>
      {(recovery.history ?? []).map((failure, index) => <div key={index} className="mono">
        {failure.model ?? t('modelRecovery.modelSlot', { slot: failure.model_slot })} · {failure.code} · {failure.request_id ?? t('modelRecovery.notSent')}
        {failure.http_status != null && ` · HTTP ${failure.http_status}`} · {failure.message}
      </div>)}
    </details>
    {canRetry && <button type="button" className="btn primary" disabled={busy}
      title={[label,shortcut].filter(Boolean).join(' · ')} onClick={() => { void retry() }}>
      {busy ? t('modelRecovery.retrying') : label}{shortcut && ` ${shortcut}`}
    </button>}
    {latestCode && SETTINGS_CODES.has(latestCode) && <button type="button" className="btn" title={t('modelRecovery.openModels')} onClick={() => openSettings('models')}>{t('modelRecovery.openModels')}</button>}
    {latestCode && USAGE_CODES.has(latestCode) && <button type="button" className="btn" title={t('modelRecovery.openUsage')} onClick={() => openSettings('usage')}>{t('modelRecovery.openUsage')}</button>}
  </div>
}
