import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches, type ActionId } from '../keymap'
import type { ExperienceHistoryKind } from '../gen/ExperienceHistoryKind'
import type { ExperienceHistoryPage } from '../gen/ExperienceHistoryPage'
import { Empty } from '@/components/ui/empty'
import { Button, buttonVariants } from '@/components/ui/button'
import { NativeSelect } from '@/components/ui/native-select'

// 功能缺口 D3（票 16B）：历史面板换 E 系列外观——类别用 A9 NativeSelect、条目逐行 12.5 + --line-soft 分隔、
// 翻页 / 刷新为 A1 xs。翻页与刷新保持原生 disabled（测试与 stale-cursor 行为按 .disabled 断言）。
const nativeBtn = buttonVariants({ size: 'xs', variant: 'default' }) + ' disabled:cursor-default disabled:opacity-45'
const linkBtn = buttonVariants({ size: 'xs', variant: 'ghost' })
const rowCls = 'border-t border-line-soft py-2 text-[12.5px] text-ink-2 first:border-t-0'

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
    <label className="mb-2 flex items-center gap-2 text-[12px] text-ink-3">{t('experience.history')}
      <NativeSelect className="h-7 w-auto min-w-[160px] text-[12.5px]" value={kind} onChange={(e) => setKind(e.target.value as ExperienceHistoryKind)}>
        {(['sources', 'versions', 'related'] as const).map((value) => <option key={value} value={value}>{t(`experience.${value}`)}</option>)}
      </NativeSelect>
    </label>
    {error && <p role="alert" className="mb-2 text-[12.5px] text-red-ink">{error}</p>}
    {/* 功能缺口 C3（A19）：说明为什么空 + 动作「刷新」（新的评审 / 修订进来后重读）。 */}
    {page?.items.length === 0 && <Empty size="sm" icon="book" title={t('experience.emptyHistory')} desc={t(`experience.emptyHistoryBody_${kind}`)}
      action={<Button size="sm" disabled={busy} onClick={() => void load(null)}>{t('experience.refreshHistory')}</Button>} />}
    {page?.items.map((item, index) => <div key={index} className={rowCls}>
      {item.kind === 'source' && <div className="flex items-center gap-2"><span className="min-w-0 flex-1">{item.source.author} · {item.source.artifact_id} · {t('experience.version', { version: item.source.artifact_version })} · {t('experience.review')} {item.source.review_event}</span>
        {item.path && <button type="button" className={linkBtn} title={title('openExperienceSource', t('experience.openSource'))}
          onKeyDown={(e) => { if (!e.repeat && matches(e.nativeEvent, bindingFor('openExperienceSource'))) { e.preventDefault(); e.stopPropagation(); void openSource(item.source.review_event) } }}
          onClick={() => void openSource(item.source.review_event)}>{t('experience.openSource')}</button>}
      </div>}
      {item.kind === 'version' && <div>{t('experience.version', { version: item.entry.revision })} · {t(`experience.${item.state}`)} · {t('experience.sourceCount', { count: item.source_count })}<p className="mt-1 whitespace-pre-wrap text-ink">{item.entry.body}</p>{item.entry.notes && <p className="text-[12px] text-ink-3">{item.entry.notes}</p>}</div>}
      {item.kind === 'control' && <div>{t(`experience.${item.operation}`)} · {t(`experience.${item.state}`)}<p className="mt-1 text-ink-3">{item.operation === 'rollback' ? t('experience.withdrawBody') : item.reason}</p></div>}
      {item.kind === 'related' && <div className="flex items-center gap-2"><span className="min-w-0 flex-1">{item.skill} · {item.entry_id} · {t(`experience.${item.state}`)}</span>
        <button type="button" className={linkBtn} title={title('openRelatedExperience', t('experience.openRelated'))}
          onKeyDown={(e) => { if (!e.repeat && matches(e.nativeEvent, bindingFor('openRelatedExperience'))) { e.preventDefault(); e.stopPropagation(); onRelated(item.skill, item.entry_id) } }}
          onClick={() => onRelated(item.skill, item.entry_id)}>{t('experience.openRelated')}</button></div>}
    </div>)}
    <div className="mt-2 flex justify-end gap-1.5">
      <button type="button" className={nativeBtn} disabled={busy} title={title('refreshExperienceHistory', t('experience.refreshHistory'))} onClick={() => void load(null)}>{t('experience.refreshHistory')}</button>
      <button type="button" className={nativeBtn} disabled={busy || !page?.next_cursor} title={title('nextExperiencePage', t('experience.nextPage'))} onClick={() => void load(page?.next_cursor ?? null)}>{t('experience.nextPage')}</button>
    </div>
    {source && <pre className="mt-2 max-h-[30vh] overflow-auto rounded-[var(--r-sm)] bg-inset p-3 font-mono text-[12px] whitespace-pre-wrap text-ink">{source}</pre>}
  </section>
}
