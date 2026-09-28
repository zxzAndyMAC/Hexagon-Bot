import { ExperienceHistory } from './ExperienceHistory'
import { ExperienceCuration } from './ExperienceCuration'
import type { ProjectSkillDocument } from '../gen/ProjectSkillDocument'
import { ExperienceLimits } from './ExperienceLimits'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { ExperienceEntryView } from '../gen/ExperienceEntryView'

export function ExperienceEntries({ skill, onRecovered, selectedEntry, documentRevision = 0 }: { skill: string; onRecovered?: () => void; selectedEntry?: string; documentRevision?: number }) {
  const { t } = useTranslation()
  const [related, setRelated] = useState<{ skill: string; entryId: string } | null>(null)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')
  const [reasons, setReasons] = useState<Record<string, string>>({})
  const [revision, setRevision] = useState(0)
  const [result, setResult] = useState<{ skill: string; entries: ExperienceEntryView[]; projectRoot: string; document: ProjectSkillDocument | null; error: string } | null>(null)
  useEffect(() => {
    let current = true
    Promise.all([api.experienceEntries(skill), api.projectSkillDocument(skill)])
      .then(([entries, document]) => { if (current) setResult({ skill, entries, projectRoot: document.project_root, document, error: '' }) })
      .catch((e) => { if (current) setResult({ skill, entries: [], projectRoot: '', document: null, error: errText(e) }) })
    return () => { current = false }
  }, [skill, revision, documentRevision])
  async function recover() {
    if (busy) return
    setBusy(true)
    setNotice('')
    try {
      const reports = await api.recoverExperience()
      setNotice(reports.filter((r) => r.state === 'conflict').map((r) => `${r.operation_id}: ${t(`experience.${r.reason_code ?? 'pending_recovery'}`)}`).join('\n') || t('experience.recovered'))
      setRevision((n) => n + 1)
      onRecovered?.()
    } catch (e) { setNotice(errText(e)) }
    finally { setBusy(false) }
  }
  async function revoke(entry: ExperienceEntryView['entry']) {
    if (busy || !result || !reasons[entry.entry_id]?.trim()) return
    setBusy(true)
    try {
      await api.revokeExperience({ project_root: result.projectRoot, skill, entry_id: entry.entry_id, expected_revision: entry.revision, reason: reasons[entry.entry_id] })
      setRevision((n) => n + 1)
      onRecovered?.()
    } catch (e) { setNotice(errText(e)) }
    finally { setBusy(false) }
  }
  if (!result || result.skill !== skill) return null
  if (result.error) return <p role="alert">{result.error}</p>
  return (
    <section aria-label={t('experience.entries')} onKeyDown={(e) => {
      // Governance 04: recovery shortcuts apply only inside this detail, never
      // behind a decision dialog or another settings panel.
      if (!e.repeat && matches(e.nativeEvent, bindingFor('recoverExperience')) && result.entries.some((r) => r.recovery_pending)) {
        e.preventDefault()
        e.stopPropagation()
        void recover()
      }
    }}>
      {result.document && <ExperienceCuration document={result.document} />}
      <ExperienceLimits onSaved={() => setRevision((n) => n + 1)} />
      {result.entries.some((r) => r.recovery_pending) && <button type="button" disabled={busy}
        title={`${t('experience.recover')} (${formatBinding(bindingFor('recoverExperience'))})`}
        onClick={() => void recover()}>{t('experience.recover')}</button>}
      {related && <aside aria-label={t('experience.related')}>
        <strong>{related.skill} · {related.entryId}</strong>
        <ExperienceEntries key={`${related.skill}:${related.entryId}`} skill={related.skill} selectedEntry={related.entryId} />
      </aside>}
      {notice && <p role="status">{notice}</p>}
      {result.entries.filter(({ entry }) => !selectedEntry || selectedEntry === entry.entry_id).map(({ entry, state, match_reason, source_count }) => (
        <article key={entry.entry_id} onKeyDown={(e) => {
          if (!e.repeat && matches(e.nativeEvent, bindingFor('revokeExperience')) && (state === 'active' || state === 'changed')) { e.preventDefault(); e.stopPropagation(); void revoke(entry) }
        }}>
          <strong>{entry.entry_id} · {t(`experience.${state}`)}</strong>
          <p>{t(`experience.${match_reason}`)}</p>
          <p style={{ whiteSpace: 'pre-wrap' }}>{entry.body}</p>
          {entry.notes && <p>{entry.notes}</p>}
          {(state === 'active' || state === 'changed') && <div>
            <label>{t('experience.revokeReason')}<input value={reasons[entry.entry_id] ?? ''} onChange={(e) => setReasons({ ...reasons, [entry.entry_id]: e.target.value })} /></label>
            <button type="button" disabled={busy || !reasons[entry.entry_id]?.trim()} onClick={() => void revoke(entry)}
              title={`${t('experience.revoke')} (${formatBinding(bindingFor('revokeExperience'))})`}>{t('experience.revoke')}</button>
          </div>}
          <div>{t('experience.version', { version: entry.revision })}</div>
          <p>{t('experience.sourceCount', { count: source_count })}</p>
          <ul>{entry.sources.slice(0, 20).map((source) => (
            <li key={`${source.artifact_id}:${source.review_event}`}>
              {source.author} · {source.artifact_id} · {t('experience.version', { version: source.artifact_version })}
            </li>
          ))}</ul>
          {source_count > 20 && <p>{t('experience.sourcesOmitted', { count: source_count - 20 })}</p>}
          <ExperienceHistory projectRoot={result.projectRoot} skill={skill} entryId={entry.entry_id} onRelated={(target, entryId) => setRelated({ skill: target, entryId })} />
        </article>
      ))}
    </section>
  )
}
