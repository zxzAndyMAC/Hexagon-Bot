import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import type { ExternalOperation } from '../gen/ExternalOperation'
import type { ManagedRecord } from '../gen/ManagedRecord'
import { TIMELINE_JUMP_EVENT } from '../timelineJump'


/** Test3/23: what delegation decided and what really happened. Only a
 * landed effect is "done"; an empty pending list is never success. */
export function ManagedReport() {
  const { t } = useTranslation()
  const [history, setHistory] = useState<ManagedRecord[]>([])
  const [external, setExternal] = useState<ExternalOperation[]>([])
  const latest = useUiStore(s => s.timelineFacts?.latest_event_id ?? 0)
  const mounted = useRef(true)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  useEffect(() => {
    void api.managedHistory().then(records => { if (mounted.current) setHistory(records) }).catch(() => {})
    void api.managedExternalOps().then(ops => { if (mounted.current) setExternal(ops) }).catch(() => {})
  }, [latest])
  if (history.length === 0) return <p className="delegation-empty">{t('delegation.empty')}</p>
  return <>
    <p className="delegation-note">{t('delegation.summary', {
      done: history.filter(r => r.result === 'done').length,
      incomplete: history.filter(r => r.result === 'incomplete').length,
      failed: history.filter(r => r.result === 'failed').length,
    })}</p>
    <ul className="delegation-list">
      {history.map(record => <li key={record.event_id} className={record.result === 'done' ? undefined : 'delegation-unfinished'}>
        <span>{t(`delegation.result.${record.result}`)}</span>
        <span>{t(`delegation.action.${record.action}`, { defaultValue: record.action })}</span>
        <span>{t(`delegation.via.${record.via}`, { defaultValue: record.via })}</span>
        <code title={record.outcome}>{record.reason}</code>
        {record.result !== 'done' && <span>{t(`delegation.next.${record.result}`)}</span>}
      </li>)}
    </ul>
    <ExternalOperations ops={external} />
  </>
}

/** Trace timestamps are SQLite UTC (`YYYY-MM-DD HH:MM:SS`); show local time. */
function localTime(at: string): string {
  const parsed = new Date(`${at.replace(' ', 'T')}Z`)
  return Number.isNaN(parsed.getTime()) ? at : parsed.toLocaleString()
}

/** Ticket 05: what delegation did outside the project. Local items moved to
 * the Trash are recoverable; app, MCP and browser actions are not verified
 * by the host, and the report says so on every entry. */
function ExternalOperations({ ops }: { ops: ExternalOperation[] }) {
  const { t } = useTranslation()
  const show = (eventId: number) => window.dispatchEvent(new CustomEvent(TIMELINE_JUMP_EVENT, { detail: eventId }))
  return <section className="delegation-external" data-external-ops>
    <h4>{t('delegation.externalTitle')}</h4>
    {ops.length === 0
      ? <p className="delegation-empty">{t('delegation.externalEmpty')}</p>
      : <ul className="delegation-list">
        {ops.map(op => <li key={op.event_id} data-external-kind={op.kind} data-verification={op.verification}
          className={op.verification === 'host_unverified_deletion' ? 'delegation-unverified' : undefined}>
          <span>{t(`delegation.externalKind.${op.kind}`, { defaultValue: op.kind })}</span>
          <code>{op.target}</code>
          {/* MCP tool names are the service's own identifiers: shown as is. */}
          <span>{t(`delegation.externalAction.${op.action}`, { defaultValue: op.action })}</span>
          <span>{t(`delegation.action.${op.decision}`, { defaultValue: op.decision })}</span>
          <span>{t(`delegation.result.${op.result}`, { defaultValue: op.result })}</span>
          <span>{t(`delegation.verification.${op.verification}`, { defaultValue: op.verification })}</span>
          {op.trash_path && <span>{t('delegation.trashedTo', { path: op.trash_path })}</span>}
          <time dateTime={op.at}>{localTime(op.at)}</time>
          <button type="button" className="btn" title={t('delegation.showInTimeline')} onClick={() => show(op.event_id)}>
            {t('delegation.showInTimeline')}
          </button>
        </li>)}
      </ul>}
  </section>
}
