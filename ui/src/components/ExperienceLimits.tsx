import { useEffect, useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { ExperienceLimits as Limits } from '../gen/ExperienceLimits'
import { Button } from '@/components/ui/button'
import { Field, Input } from '@/components/ui/input'

// 功能缺口 D3（票 16B）：原生 <fieldset>/<legend> 换成 A9 字段三列 + 「保存限制」A1 sm；标题由外层折叠行给。
export function ExperienceLimits({ onSaved }: { onSaved: () => void }) {
  const { t } = useTranslation()
  const [limits, setLimits] = useState<Limits | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const id = useId()
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
  return <div role="group" aria-label={t('experience.limits')} data-experience-limits onKeyDown={(e) => {
    if (!e.repeat && matches(e.nativeEvent, bindingFor('saveExperienceLimits'))) { e.preventDefault(); e.stopPropagation(); void save() }
  }}>
    <div className="grid grid-cols-3 gap-3">
      {limits && (['entry_chars', 'load_count', 'load_chars'] as const).map((key) => <Field key={key} label={t(`experience.${key}`)} htmlFor={`${id}-${key}`}>
        <Input id={`${id}-${key}`} type="number" min={1} step={1} className="font-mono" value={limits[key]} onChange={(e) => setLimits({ ...limits, [key]: Number(e.target.value) })} />
      </Field>)}
    </div>
    <div className="mt-3 flex items-center gap-2">
      {error ? <p role="alert" className="min-w-0 flex-1 text-[12px] text-red">{error}</p> : <span className="flex-1" />}
      <Button size="sm" disabled={busy || !limits} loading={busy} onClick={() => void save()}
        title={`${t('experience.saveLimits')} (${formatBinding(bindingFor('saveExperienceLimits'))})`}>{t('experience.saveLimits')}</Button>
    </div>
  </div>
}
