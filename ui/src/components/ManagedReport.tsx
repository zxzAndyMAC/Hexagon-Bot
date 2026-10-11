import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import type { ManagedRecord } from '../gen/ManagedRecord'

/** Test3/23: what delegation decided and what really happened. Only a
 * landed effect is "done"; an empty pending list is never success. */
export function ManagedReport() {
  const { t } = useTranslation()
  const [history, setHistory] = useState<ManagedRecord[]>([])
  const latest = useUiStore(s => s.timelineFacts?.latest_event_id ?? 0)
  const mounted = useRef(true)
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  useEffect(() => {
    void api.managedHistory().then(records => { if (mounted.current) setHistory(records) }).catch(() => {})
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
  </>
}
