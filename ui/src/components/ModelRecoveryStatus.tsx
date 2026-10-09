import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'
import type { ModelRequestRecovery } from '../gen/ModelRequestRecovery'

// MR-01: both surfaces describe the same persisted item. Only the original
// failed closed choice can retry; generic stage recovery must never receive it.
export function ModelRecoveryStatus({ id, payload, top = false }: { id: string; payload: Record<string, unknown>; top?: boolean }) {
  const { t } = useTranslation()
  const recovery = (payload.view ?? payload) as ModelRequestRecovery
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)
  const recovered = recovery.state === 'recovered'
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
  const shortcut = top ? formatBinding(bindingFor('approve')) : ''
  return <div data-model-recovery={id} aria-busy={busy}>
    <div style={{fontWeight: 510, color: recovered ? 'var(--ok)' : 'var(--accent)'}}>{t(recovered ? 'modelRecovery.recovered' : 'modelRecovery.title')}</div>
    <div className="dim3" style={{fontSize: 12, margin: '4px 0 8px'}}>
      {t(recovered ? 'modelRecovery.recoveredHint' : 'modelRecovery.hint', { target: recovery.target_role ?? t('modelRecovery.nextRole') })}
    </div>
    <details className="dim3" style={{fontSize: 11, marginBottom: 8}}>
      <summary>{t('modelRecovery.details')}</summary>
      {(recovery.history ?? []).map((failure, index) => <div key={index} className="mono">
        {failure.model ?? t('modelRecovery.modelSlot', { slot: failure.model_slot })} · {failure.code} · {failure.request_id ?? t('modelRecovery.notSent')}
        {failure.http_status != null && ` · HTTP ${failure.http_status}`} · {failure.message}
      </div>)}
    </details>
    {!recovered && <button type="button" className="btn primary" disabled={busy}
      title={[label,shortcut].filter(Boolean).join(' · ')} onClick={() => { void retry() }}>
      {busy ? t('modelRecovery.retrying') : label}{shortcut && ` ${shortcut}`}
    </button>}
  </div>
}
