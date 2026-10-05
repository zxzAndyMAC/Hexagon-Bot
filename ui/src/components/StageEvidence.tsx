import { useRef, useState } from 'react'
import { exceptionLabel } from '../evidenceLabels'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useStageEvidence } from '../useStageEvidence'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'
import { PerformanceBaselineReview } from './PerformanceBaselineReview'

/** D08: render core's current evidence, never infer success from an old exit code. */
export function StageEvidence({ runId }: { runId?: string }) {
  const { t } = useTranslation()
  const requestButton = useRef<HTMLButtonElement>(null)
  const [busy, setBusy] = useState(false)
  const requirement = (item: string) => {
    const colon = item.indexOf(':')
    const kind = item.slice(0, colon)
    if (kind === 'quality') return t('quality.required', { category: t(`quality.category.${item.slice(colon + 1)}`) })
    if (kind === 'check' && item.slice(colon + 1).startsWith('quality:')) return t('quality.required', { category: t(`quality.category.${item.slice(colon + 1 + 'quality:'.length)}`) })
    if (kind === 'delivery') return t('evidence.deliveryChanged')
    if (['artifact', 'review', 'check', 'stage', 'action'].includes(kind)) {
      return t(`evidence.requirement_${kind}`, { value: item.slice(colon + 1) })
    }
    return item
  }
  const { evidence, error } = useStageEvidence(runId)
  if (error) return <div role="status" className="dim" style={{ fontSize: 11 }}>{t('evidence.unavailable')} · {error}</div>
  if (!evidence || (runId && evidence.run_id !== runId)) return null
  return (
    <section onKeyDown={(e) => {
      if (!matches(e, bindingFor('requestException'))) return
      e.preventDefault()
      if (!e.repeat && useUiStore.getState().modalScope === 'workbench') requestButton.current?.click()
    }} aria-label={t('evidence.title')} style={{ padding: '8px 0', fontSize: 12 }}>
      <div style={{ fontWeight: 510 }}>{t('evidence.title')}</div>
      {evidence.checks.length === 0 && <div className="dim3">{t('evidence.noChecks')}</div>}
      {evidence.checks.map((check) => (
        <div key={`${check.run_id}:${check.cmd}`} style={{ marginTop: 6 }}>
          <div className="mono" style={{ overflowWrap: 'anywhere' }}>{check.stage} · {check.quality ? t(`quality.category.${check.quality.category}`) : check.cmd}</div>
          <span className={`chip ${check.state === 'passed' ? 'ok' : 'warn'}`}>{t(`evidence.${check.state}`)}</span>
          {/* Live event 56 (2026-10-01): quality -2 is a regression verdict,
              not the original process exit (which was 0). Show the measured
              quality reason below; raw command checks keep their exit code. */}
          {check.exit_code !== null && !check.quality && <span className="dim3"> · {t('evidence.exit', { code: check.exit_code })}</span>}
          {check.quality && <div className="quality-evidence">
            <span>{t(`quality.category.${check.quality.category}`)} · {t(`quality.reason.${check.quality.reason}`)}</span>
            {check.quality.measured_ms != null && <div>{t('quality.measurement', { value: check.quality.measured_ms })}</div>}
            {check.quality.baseline_ms != null && <div>{t('quality.baseline', { value: check.quality.baseline_ms })}</div>}
            <details><summary>{t('quality.details')}</summary>
              {check.quality.command && <div className="mono">{check.quality.command}</div>}
              <div className="dim3">{t('quality.changed', { count: check.quality.changed_paths.length })}</div>
            </details>
            {check.quality.reason === 'measurement_changed' && check.state === 'failed' && check.event_id != null && evidence.fingerprint &&
              <PerformanceBaselineReview key={`${check.event_id}:${evidence.fingerprint}`} measurement={check.event_id} expected={evidence.fingerprint} />}
          </div>}
        </div>
      ))}
      {evidence.exceptions.filter((item) => item.accepted).map((item) => (
        <div key={JSON.stringify(item.requirement)} className="chip warn">{t('exceptions.accepted')} · {exceptionLabel(item)}</div>
      ))}
      {evidence.exceptions.some((item) => !item.accepted) && <button
        ref={requestButton} className="btn" disabled={busy || !evidence.fingerprint || evidence.missing.some((m) => m.startsWith('action:'))}
        title={`${t('exceptions.request')} · ${formatBinding(bindingFor('requestException'))}`}
        onClick={async () => {
          if (!evidence.fingerprint) return
          setBusy(true)
          try {
            await api.requestAcceptanceException(evidence.fingerprint)
            await useUiStore.getState().invalidate()
            useUiStore.getState().openPendingDialog()
          } catch (e) { useUiStore.getState().pushToast(errText(e), 'err') }
          finally { setBusy(false) }
        }}
      >{t('exceptions.request')}</button>}
      {evidence.missing.length > 0 && (
        <div className="dim" style={{ marginTop: 6 }}>
          {t('evidence.missingRequirements')}
          {evidence.missing.map((item, index) => <div key={`${index}:${item}`} className="mono" style={{ overflowWrap: 'anywhere' }}>{requirement(item)}</div>)}
        </div>
      )}
    </section>
  )
}
