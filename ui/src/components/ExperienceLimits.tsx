import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { ExperienceLimits as Limits } from '../gen/ExperienceLimits'

export function ExperienceLimits({ onSaved }: { onSaved: () => void }) {
  const { t } = useTranslation()
  const [limits, setLimits] = useState<Limits | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    let current = true
    api.experienceLimits().then((value) => { if (current) setLimits(value) }).catch((e) => { if (current) setError(errText(e)) })
    return () => { current = false }
  }, [])
  async function save() {
    if (!limits || busy) return
    setBusy(true)
    setError('')
    try { setLimits(await api.setExperienceLimits(limits)); onSaved() }
    catch (e) { setError(errText(e)) }
    finally { setBusy(false) }
  }
  return <fieldset onKeyDown={(e) => {
    if (!e.repeat && matches(e.nativeEvent, bindingFor('saveExperienceLimits'))) { e.preventDefault(); e.stopPropagation(); void save() }
  }}>
    <legend>{t('experience.limits')}</legend>
    {limits && (['entry_chars', 'load_count', 'load_chars'] as const).map((key) => <label key={key}>
      {t(`experience.${key}`)}
      <input type="number" min={1} step={1} value={limits[key]} onChange={(e) => setLimits({ ...limits, [key]: Number(e.target.value) })} />
    </label>)}
    <button type="button" disabled={busy || !limits} onClick={() => void save()}
      title={`${t('experience.saveLimits')} (${formatBinding(bindingFor('saveExperienceLimits'))})`}>{t('experience.saveLimits')}</button>
    {error && <p role="alert">{error}</p>}
  </fieldset>
}
