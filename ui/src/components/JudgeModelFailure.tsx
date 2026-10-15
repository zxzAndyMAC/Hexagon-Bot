import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type PendingQuestion } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'
import { Icon } from './Icon'
import { DecisionButton } from './DecisionShell'

type Failure = { purpose?: string; code?: string; request_id?: string | null; model_slot?: string }

export function judgeFailureOf(q: PendingQuestion): Failure | undefined {
  return q.payload.model_failure as Failure | undefined
}

// MR-06 (test3 10): an internal judge request failed and the card fell back
// to the owner. Say plainly it is a request failure, not a verdict; the
// card's own decision buttons stay the owner's path. Re-judge re-checks the
// original target in core and grants nothing a first success could not.
// 票 04（components D8「自动判定请求失败」）：卡头改写成「…请求失败，交给你决定」+ 副行「这不是判定结论 · 码」，
// 「重新判定」是底栏左侧 ghost 次动作（RejudgeButton），本块只留技术细节行。
export function JudgeModelFailure({ q }: { q: PendingQuestion; top?: boolean }) {
  const { t } = useTranslation()
  const failure = judgeFailureOf(q)
  const advice = (q.payload.judge_reason as { code?: string; failure?: string } | undefined)
  if (!failure && advice?.code !== 'model_request_failed') return null
  if (!failure) {
    return <div className="dock-hint" data-judge-failure="advice">
      {t('judgeFailure.advice', { code: advice?.failure ?? '' })}
    </div>
  }
  return <div data-judge-failure={failure.purpose} className="dock-hint">
    <div>{t('judgeFailure.hint')}</div>
    <div className="mono">{failure.code} · {failure.model_slot} · {failure.request_id ?? t('modelRecovery.notSent')}</div>
  </div>
}

export function judgeFailureTitle(q: PendingQuestion, t: (k: string, o?: Record<string, unknown>) => string): string | null {
  const failure = judgeFailureOf(q)
  if (!failure) return null
  return t('judgeFailure.title', { purpose: t(`judgeFailure.purpose.${failure.purpose}`, { defaultValue: failure.purpose ?? '' }) })
}

export function RejudgeButton({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const failure = judgeFailureOf(q)
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
  if (!failure) return null
  return <DecisionButton variant="ghost" busy={busy}
    title={[t('judgeFailure.rejudgeHint'), top ? formatBinding(bindingFor('rejudgeAfterModelFailure')) : ''].filter(Boolean).join(' · ')}
    onClick={() => { void rejudge() }}><Icon name="refresh" size={12} />{t('judgeFailure.rejudge')}</DecisionButton>
}
