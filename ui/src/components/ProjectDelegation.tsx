import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { DelegationStatus } from '../gen/DelegationStatus'
import type { ManagedRecord } from '../gen/ManagedRecord'
import './ProjectDelegation.css'

/** Test3/23: a decision is only "done" when its real effect landed; an
 * empty pending list is never reported as business success. */
function resultOf(record: ManagedRecord): 'done' | 'incomplete' | 'failed' {
  if (record.outcome.startsWith('failed')) return 'failed'
  if (record.outcome === 'incomplete' || record.action === 'incomplete' || record.outcome === 'still_incomplete') return 'incomplete'
  return 'done'
}

/** Test3/17: project delegation is a separate owner switch, never a new
 * meaning of an approval mode. Turning it on always needs an explicit second
 * click; the host persists the grant before the UI claims it is on. */
export function ProjectDelegation() {
  const { t } = useTranslation()
  const [status, setStatus] = useState<DelegationStatus | null>(null)
  const [root, setRoot] = useState<string | null>(null)
  const [history, setHistory] = useState<ManagedRecord[]>([])
  const [processing, setProcessing] = useState(0)
  const [confirming, setConfirming] = useState(false)
  const [pending, setPending] = useState(false)
  const [failed, setFailed] = useState(false)
  const latest = useUiStore(s => s.timelineFacts?.latest_event_id ?? 0)
  const mounted = useRef(true)
  const toggle = useRef<HTMLButtonElement>(null)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  useEffect(() => {
    void Promise.all([api.projectDelegation(), api.approvalMode(), api.managedHistory(), api.managedProcessing()]).then(([value, mode, records, held]) => {
      if (!mounted.current) return
      setStatus(value); setRoot(mode.project_root ?? null); setHistory(records); setProcessing(held.length)
    }).catch(() => { if (mounted.current) setFailed(true) })
  }, [latest])
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (!event.defaultPrevented && matches(event, bindingFor('projectDelegation'))) {
        event.preventDefault(); toggle.current?.click()
      }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [])
  async function save(enabled: boolean) {
    if (!root || pending) return
    setPending(true); setFailed(false)
    try {
      const saved = await api.setProjectDelegation(enabled, root)
      if (mounted.current) { setStatus(saved); setConfirming(false) }
    } catch {
      if (mounted.current) setFailed(true)
    } finally {
      if (mounted.current) setPending(false)
    }
  }
  const on = status?.enabled ?? false
  const shortcut = formatBinding(bindingFor('projectDelegation'))
  return <section className="delegation" aria-label={t('delegation.label')}>
    <div className="delegation-head">
      <strong>{t('delegation.label')}</strong>
      <button ref={toggle} type="button" className={`approval-mode-trigger${on ? ' delegation-on' : ''}`}
        disabled={!status || !root || pending} aria-pressed={on}
        title={[t(on ? 'delegation.turnOff' : 'delegation.turnOn'), shortcut].filter(Boolean).join(' · ')}
        onClick={() => { if (on) void save(false); else setConfirming(true) }}>
        {t(on ? 'delegation.on' : 'delegation.off')}
      </button>
    </div>
    <p className="delegation-note">{t('delegation.description')}</p>
    {confirming && <div className="delegation-confirm" role="alertdialog" aria-label={t('delegation.confirmTitle')}>
      <p className="delegation-note">{t('delegation.confirmBody')}</p>
      <div className="delegation-confirm-actions">
        <button type="button" className="approval-mode-trigger" onClick={() => setConfirming(false)}>{t('agent.cancel')}</button>
        <button type="button" className="approval-mode-trigger delegation-on" disabled={pending} onClick={() => void save(true)}>{t('delegation.confirmAction')}</button>
      </div>
    </div>}
    {on && processing > 0 && <p className="delegation-note" role="status">{t('delegation.processing', { count: processing })}</p>}
    {failed && <p className="delegation-error" role="alert">{t('delegation.failed')}</p>}
    {history.length === 0 ? <p className="delegation-empty">{t('delegation.empty')}</p> : <ul className="delegation-list">
      {history.map(record => {
        const result = resultOf(record)
        return <li key={record.event_id} className={result === 'done' ? undefined : 'delegation-unfinished'}>
          <span>{t(`delegation.result.${result}`)}</span>
          <span>{t(`delegation.action.${record.action}`, { defaultValue: record.action })}</span>
          <span>{t(`delegation.via.${record.via}`, { defaultValue: record.via })}</span>
          <code title={record.outcome}>{record.reason}</code>
        </li>
      })}
    </ul>}
  </section>
}
