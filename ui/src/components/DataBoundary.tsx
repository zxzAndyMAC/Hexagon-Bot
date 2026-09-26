import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import type { DataBoundary as Boundary } from '../gen/DataBoundary'

/** Reliability 22: a read-only explanation of actual storage and configured exits. */
export function DataBoundary() {
  const { t } = useTranslation()
  const [value, setValue] = useState<Boundary | null>(null)
  const [failed, setFailed] = useState(false)
  useEffect(() => {
    let live = true
    api.dataBoundary().then((v) => { if (live) setValue(v) }).catch(() => { if (live) setFailed(true) })
    return () => { live = false }
  }, [])
  return <section className="panel" style={{ padding: 16, marginTop: 20, fontSize: 12, lineHeight: 1.8 }}>
    <h3>{t('dataBoundary.title')}</h3>
    <p>{t('dataBoundary.local')}</p>
    <p>{t('dataBoundary.model')}</p>
    <p>{t('dataBoundary.mcp')}</p>
    <p>{t('dataBoundary.mcpStorage')}</p>
    <p>{t('dataBoundary.terminal')}</p>
    <p>{t('dataBoundary.retention')}</p>
    {failed ? <p role="alert">{t('dataBoundary.unavailable')}</p> : value && <>
      <p><b>{t('dataBoundary.credentials')}: </b>{t(`dataBoundary.backend_${value.credential_backend}`, { defaultValue: t('dataBoundary.backend_custom') })}</p>
      <p>{t('dataBoundary.configured')}</p>
      {value.recipients.length === 0 ? <p>{t('dataBoundary.empty')}</p> : <ul>
        {value.recipients.map((r, i) => <li key={i}>
          {t(`dataBoundary.kind_${r.kind}`)} · {r.endpoint ?? t(r.transport === 'stdio' ? 'dataBoundary.localProcess' : 'dataBoundary.unknownEndpoint')} · {t(r.enabled ? 'dataBoundary.enabled' : 'dataBoundary.disabled')}
        </li>)}
      </ul>}
    </>}
  </section>
}
