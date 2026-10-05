import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'

/** Q2: one explicit owner confirmation bound to the displayed real measurement. */
export function PerformanceBaselineReview({ measurement, expected }: { measurement: number; expected: string }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState(false)
  const [confirmed, setConfirmed] = useState(false)
  const review = useRef<HTMLButtonElement>(null)
  const confirm = useRef<HTMLButtonElement>(null)
  const cancel = useRef<HTMLButtonElement>(null)
  return <div onKeyDown={e => {
    for (const [action, button] of [['reviewPerformanceBaseline', review], ['confirmPerformanceBaseline', confirm], ['cancelPerformanceBaseline', cancel]] as const) {
      if (matches(e.nativeEvent, bindingFor(action)) && button.current) {
        e.preventDefault(); e.stopPropagation(); if (!e.repeat) button.current.click(); return
      }
    }
  }}>
    {confirmed ? <div role="status">{t('quality.baselineConfirmed')}</div> : !open ? <button ref={review} className="btn"
      title={`${t('quality.reviewBaseline')} · ${formatBinding(bindingFor('reviewPerformanceBaseline'))}`}
      onClick={() => setOpen(true)}>{t('quality.reviewBaseline')}</button> : <form onSubmit={async e => {
        e.preventDefault(); if (busy || !reason.trim()) return
        setBusy(true)
        try {
          await api.confirmPerformanceBaseline(measurement, expected, reason.trim())
          setConfirmed(true); setOpen(false)
          await useUiStore.getState().invalidate()
        } catch (error) { useUiStore.getState().pushToast(errText(error), 'err') }
        finally { setBusy(false) }
      }}>
      <p>{t('quality.baselineConfirmation')}</p>
      <label>{t('quality.baselineReason')}<input value={reason} disabled={busy} onChange={e => setReason(e.target.value)} /></label>
      <button ref={confirm} type="submit" className="btn" disabled={busy || !reason.trim()}
        title={`${t('quality.confirmBaseline')} · ${formatBinding(bindingFor('confirmPerformanceBaseline'))}`}>{t('quality.confirmBaseline')}</button>
      <button ref={cancel} type="button" className="btn" disabled={busy} onClick={() => setOpen(false)}
        title={`${t('quality.cancelBaseline')} · ${formatBinding(bindingFor('cancelPerformanceBaseline'))}`}>{t('quality.cancelBaseline')}</button>
    </form>}
  </div>
}
