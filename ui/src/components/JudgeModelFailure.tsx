import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type PendingQuestion } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'

// MR-06 (test3 10): an internal judge request failed and the card fell back
// to the owner. Say plainly it is a request failure, not a verdict; the
// card's own decision buttons stay the owner's path. Re-judge re-checks the
// original target in core and grants nothing a first success could not.
export function JudgeModelFailure({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const failure = q.payload.model_failure as { purpose?: string; code?: string; request_id?: string | null; model_slot?: string } | undefined
  const advice = (q.payload.judge_reason as { code?: string; failure?: string } | undefined)
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)
  const rejudge = async () => {
    if (inFlight.current || !failure) return
    const store = useUiStore.getState()
    if (store.modalScope !== 'workbench') { store.pushToast(t('decisions.scopeBlocked')); return }
    inFlight.current = true; setBusy(true)
    try {
      const outcome = await api.rejudgeAfterModelFailure(q.id, store.projectRoot ?? '')
      store.pushToast(t(`judgeFailure.outcome.${outcome}`))
    } catch (error) {
      store.pushToast(t('errors.actionFailed', { detail: errText(error) }), 'err')
    } finally {
      try { await useUiStore.getState().invalidate() } finally { inFlight.current = false; setBusy(false) }
    }
  }
  useEffect(() => {
    if (!top || !failure) return
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat || e.defaultPrevented || !matches(e, bindingFor('rejudgeAfterModelFailure'))) return
      e.preventDefault(); void rejudge()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })
  if (!failure && advice?.code !== 'model_request_failed') return null
  if (!failure) {
    return <div className="dim3" data-judge-failure="advice" style={{fontSize: 11, margin: '4px 0'}}>
      {t('judgeFailure.advice', { code: advice?.failure ?? '' })}
    </div>
  }
  const label = t('judgeFailure.rejudge')
  return <div data-judge-failure={failure.purpose} style={{fontSize: 12, margin: '4px 0 8px'}}>
    <div style={{fontWeight: 510, color: 'var(--accent)'}}>{t('judgeFailure.title', { purpose: t(`judgeFailure.purpose.${failure.purpose}`, { defaultValue: failure.purpose ?? '' }) })}</div>
    <div className="dim3" style={{margin: '2px 0 6px'}}>{t('judgeFailure.hint')}</div>
    <div className="dim3 mono" style={{fontSize: 11, marginBottom: 6}}>{failure.code} · {failure.model_slot} · {failure.request_id ?? t('modelRecovery.notSent')}</div>
    <button type="button" className="btn" disabled={busy}
      title={[t('judgeFailure.rejudgeHint'), top ? formatBinding(bindingFor('rejudgeAfterModelFailure')) : ''].filter(Boolean).join(' · ')}
      onClick={() => { void rejudge() }}>{label}</button>
  </div>
}
