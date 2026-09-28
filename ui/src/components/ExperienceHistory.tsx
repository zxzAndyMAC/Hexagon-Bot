import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches, type ActionId } from '../keymap'
import type { ExperienceHistoryKind } from '../gen/ExperienceHistoryKind'
import type { ExperienceHistoryPage } from '../gen/ExperienceHistoryPage'

export function ExperienceHistory({ projectRoot, skill, entryId, onRelated }: {
  projectRoot: string; skill: string; entryId: string; onRelated: (skill: string, entryId: string) => void
}) {
  const { t } = useTranslation()
  const [kind, setKind] = useState<ExperienceHistoryKind>('sources')
  const [page, setPage] = useState<ExperienceHistoryPage | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [source, setSource] = useState('')
  const generation = useRef(0)
  const sourceGeneration = useRef(0)
  async function load(cursor: string | null) {
    const token = ++generation.current
    setBusy(true); setError('')
    try {
      const next = await api.experienceHistory({ project_root: projectRoot, skill, entry_id: entryId, kind, cursor, limit: 20 })
      if (token === generation.current) setPage(next)
    } catch (e) { if (token === generation.current) setError(errText(e)) }
    finally { if (token === generation.current) setBusy(false) }
  }
  useEffect(() => {
    setPage(null); setSource(''); void load(null)
    return () => { generation.current++ }
    // Scope/kind changes invalidate in-flight results, including source reads.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectRoot, skill, entryId, kind])
  const title = (id: ActionId, label: string) => `${label} (${formatBinding(bindingFor(id))})`
  async function openSource(reviewEvent: number) {
    const token = generation.current
    const sourceToken = ++sourceGeneration.current
    setSource('')
    try {
      const document = await api.experienceSourceDocument({ project_root: projectRoot, skill, entry_id: entryId, review_event: reviewEvent })
      if (token === generation.current && sourceToken === sourceGeneration.current) setSource(document.content)
    } catch (e) { if (token === generation.current && sourceToken === sourceGeneration.current) setError(errText(e)) }
  }
  return <section aria-label={t('experience.history')} onKeyDown={(e) => {
    if (e.repeat) return
    if (matches(e.nativeEvent, bindingFor('nextExperiencePage')) && page?.next_cursor && !busy) {
      e.preventDefault(); e.stopPropagation(); void load(page.next_cursor)
    } else if (matches(e.nativeEvent, bindingFor('refreshExperienceHistory')) && !busy) {
      e.preventDefault(); e.stopPropagation(); void load(null)
    }
  }}>
    <label>{t('experience.history')}<select value={kind} onChange={(e) => setKind(e.target.value as ExperienceHistoryKind)}>
      {(['sources', 'versions', 'related'] as const).map((value) => <option key={value} value={value}>{t(`experience.${value}`)}</option>)}
    </select></label>
    {error && <p role="alert">{error}</p>}
    {page?.items.length === 0 && <p>{t('experience.emptyHistory')}</p>}
    {page?.items.map((item, index) => <div key={index}>
      {item.kind === 'source' && <div>{item.source.author} · {item.source.artifact_id} · {t('experience.version', { version: item.source.artifact_version })} · {t('experience.review')} {item.source.review_event}
        {item.path && <button type="button" title={title('openExperienceSource', t('experience.openSource'))}
          onKeyDown={(e) => { if (!e.repeat && matches(e.nativeEvent, bindingFor('openExperienceSource'))) { e.preventDefault(); e.stopPropagation(); void openSource(item.source.review_event) } }}
          onClick={() => void openSource(item.source.review_event)}>{t('experience.openSource')}</button>}
      </div>}
      {item.kind === 'version' && <div>{t('experience.version', { version: item.entry.revision })} · {t(`experience.${item.state}`)} · {t('experience.sourceCount', { count: item.source_count })}<p>{item.entry.body}</p><p>{item.entry.notes}</p></div>}
      {item.kind === 'control' && <div>{t(`experience.${item.operation}`)} · {t(`experience.${item.state}`)}<p>{item.operation === 'rollback' ? t('experience.withdrawBody') : item.reason}</p></div>}
      {item.kind === 'related' && <div>{item.skill} · {item.entry_id} · {t(`experience.${item.state}`)}
        <button type="button" title={title('openRelatedExperience', t('experience.openRelated'))}
          onKeyDown={(e) => { if (!e.repeat && matches(e.nativeEvent, bindingFor('openRelatedExperience'))) { e.preventDefault(); e.stopPropagation(); onRelated(item.skill, item.entry_id) } }}
          onClick={() => onRelated(item.skill, item.entry_id)}>{t('experience.openRelated')}</button></div>}
    </div>)}
    <button type="button" disabled={busy} title={title('refreshExperienceHistory', t('experience.refreshHistory'))} onClick={() => void load(null)}>{t('experience.refreshHistory')}</button>
    <button type="button" disabled={busy || !page?.next_cursor} title={title('nextExperiencePage', t('experience.nextPage'))} onClick={() => void load(page?.next_cursor ?? null)}>{t('experience.nextPage')}</button>
    {source && <pre>{source}</pre>}
  </section>
}
