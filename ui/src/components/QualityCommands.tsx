import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import type { QualityConfiguration } from '../gen/QualityConfiguration'
import type { QualityCategory } from '../gen/QualityCategory'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'

const categories: QualityCategory[] = ['tests', 'performance', 'accessibility', 'security']

/** Q4: a dedicated owner confirmation for the current flow. */
export function QualityCommands() {
  const { t } = useTranslation()
  const [config, setConfig] = useState<QualityConfiguration | null>(null)
  const [seq, setSeq] = useState(0)
  const [draft, setDraft] = useState<Partial<Record<QualityCategory, string>>>({})
  const [reviewing, setReviewing] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [saved, setSaved] = useState(false)
  const [checked, setChecked] = useState(false)
  const review = useRef<HTMLButtonElement>(null)
  const confirm = useRef<HTMLButtonElement>(null)
  const cancel = useRef<HTMLButtonElement>(null)
  const run = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    let live = true
    api.qualityConfiguration().then(value => {
      if (!live) return
      setConfig(value); setSeq(value.stages[0]?.seq ?? 0); setDraft(value.stages[0]?.commands ?? {})
    }).catch(e => { if (live) setError(errText(e)) })
    return () => { live = false }
  }, [])
  const stage = config?.stages.find(value => value.seq === seq)
  const commands = Object.fromEntries(categories.filter(category => draft[category]?.trim()).map(category => [category, draft[category]!.trim()]))
  const changed = categories.filter(category => (commands[category] ?? '') !== (stage?.commands[category] ?? ''))
  const title = (action: 'reviewQualityCommands' | 'confirmQualityCommands' | 'cancelQualityCommands' | 'runQualityChecks', key: string) => `${t(key)} · ${formatBinding(bindingFor(action))}`
  return <section onKeyDown={e => {
    for (const [action, button] of [['reviewQualityCommands', review], ['confirmQualityCommands', confirm], ['cancelQualityCommands', cancel], ['runQualityChecks', run]] as const) {
      if (matches(e.nativeEvent, bindingFor(action)) && button.current) {
        e.preventDefault(); e.stopPropagation(); if (!e.repeat) button.current.click(); return
      }
    }
  }}>
    <h3>{t('quality.configTitle')}</h3>
    <p className="dim3">{t('quality.configHint')}</p>
    {error && <div role="alert">{error}</div>}
    {saved && <div role="status">{t(checked ? 'quality.configChecked' : 'quality.configSaved')}
      <button ref={run} className="btn" disabled={busy} title={title('runQualityChecks', 'quality.configRun')} onClick={async () => {
        if (!config || busy) return
        setBusy(true); setError('')
        try { await api.runChecks(config.project_root); setChecked(true); await useUiStore.getState().invalidate() }
        catch (e) { setError(errText(e)) }
        finally { setBusy(false) }
      }}>{t('quality.configRun')}</button>
    </div>}
    {stage && <>
      <label>{t('quality.configStage')}<select value={seq} disabled={reviewing || busy} onChange={e => {
        const next = config!.stages.find(value => value.seq === Number(e.target.value))!
        setSeq(next.seq); setDraft(next.commands); setSaved(false); setError('')
      }}>{config!.stages.map(value => <option key={value.seq} value={value.seq}>{value.name}</option>)}</select></label>
      {!reviewing ? <>
        {categories.map(category => <label key={category} style={{ display: 'block', marginTop: 8 }}>{t(`quality.category.${category}`)}
          <input data-quality-category={category} value={draft[category] ?? ''} disabled={busy}
            onChange={e => { setDraft(value => ({ ...value, [category]: e.target.value })); setSaved(false) }} />
        </label>)}
        <button ref={review} data-review-quality className="btn" disabled={busy || !changed.length}
          title={title('reviewQualityCommands', 'quality.configReview')} onClick={() => { setReviewing(true); setSaved(false) }}>{t('quality.configReview')}</button>
      </> : <>
        <p>{t('quality.configImpact')}</p>
        <ul>{changed.map(category => <li key={category}>{t(`quality.category.${category}`)}: <code>{stage.commands[category] || t('quality.configBlank')}</code> → <code>{commands[category] || t('quality.configBlank')}</code></li>)}</ul>
        <button ref={confirm} data-confirm-quality className="btn" disabled={busy || !changed.length}
          title={title('confirmQualityCommands', 'quality.configConfirm')} onClick={async () => {
            if (busy || !config) return
            setBusy(true); setError('')
            try {
              const next = await api.updateQualityCommands(seq, config.version, commands, config.project_root)
              setConfig(next); setDraft(next.stages.find(value => value.seq === seq)!.commands); setReviewing(false); setSaved(true); setChecked(false)
              await useUiStore.getState().invalidate()
            } catch (e) {
              setError(errText(e)); setReviewing(false)
              // An expired confirmation must be reviewed against freshly read
              // commands. Keep the owner's draft; never silently resubmit it.
              try { setConfig(await api.qualityConfiguration()) } catch { /* original error remains visible */ }
            }
            finally { setBusy(false) }
          }}>{t('quality.configConfirm')}</button>
        <button ref={cancel} className="btn" disabled={busy} title={title('cancelQualityCommands', 'quality.configCancel')}
          onClick={() => setReviewing(false)}>{t('quality.configCancel')}</button>
      </>}
    </>}
  </section>
}
