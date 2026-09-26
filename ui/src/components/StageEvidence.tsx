import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type StageEvidence as Evidence } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'

/** D08: render core's current evidence, never infer success from an old exit code. */
export function StageEvidence({ runId }: { runId?: string }) {
  const { t } = useTranslation()
  const requestButton = useRef<HTMLButtonElement>(null)
  const [busy, setBusy] = useState(false)
  const requirement = (item: string) => {
    const colon = item.indexOf(':')
    const kind = item.slice(0, colon)
    if (kind === 'delivery') return t('evidence.deliveryChanged')
    if (['artifact', 'review', 'check', 'stage', 'action'].includes(kind)) {
      return t(`evidence.requirement_${kind}`, { value: item.slice(colon + 1) })
    }
    return item
  }
  const revision = useUiStore((s) => s.evidenceRevision)
  const eventId = useUiStore((s) => s.timeline.at(-1)?.event.id)
  const activeRun = useUiStore((s) => s.stages.find((r) => r.state === 'active' || r.state === 'waiting_stamp')?.run_id)
  const [evidence, setEvidence] = useState<Evidence | null>(null)
  const [error, setError] = useState('')
  useEffect(() => {
    let disposed = false
    let request = 0
    const refresh = async () => {
      const current = ++request
      setEvidence(null)
      setError('')
      try {
        const next = await api.stageEvidence()
        if (!disposed && current === request) setEvidence(next)
      } catch (e) {
        if (!disposed && current === request) setError(errText(e))
      }
    }
    void refresh()
    window.addEventListener('focus', refresh)
    return () => { disposed = true; window.removeEventListener('focus', refresh) }
  }, [runId, activeRun, eventId, revision])
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
          <div className="mono" style={{ overflowWrap: 'anywhere' }}>{check.stage} · {check.cmd}</div>
          <span className={`chip ${check.state === 'passed' ? 'ok' : 'warn'}`}>{t(`evidence.${check.state}`)}</span>
          {check.exit_code !== null && <span className="dim3"> · {t('evidence.exit', { code: check.exit_code })}</span>}
        </div>
      ))}
      {evidence.exceptions.filter((item) => item.accepted).map((item) => (
        <div key={JSON.stringify(item.requirement)} className="chip warn">{t('exceptions.accepted')} · {item.label}</div>
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
