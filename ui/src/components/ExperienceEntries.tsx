import { ExperienceHistory } from './ExperienceHistory'
import { ExperienceCuration } from './ExperienceCuration'
import type { ProjectSkillDocument } from '../gen/ProjectSkillDocument'
import { ExperienceLimits } from './ExperienceLimits'
import { useEffect, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { ExperienceEntryView } from '../gen/ExperienceEntryView'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { Empty } from '@/components/ui/empty'
import { Icon } from './Icon'

// ui-redesign 票 11 → 功能缺口 D3（票 16B settings#skills，原型 settings.html「经验条目（2）」）：
// 小节头「经验条目（N）」+ ghost「历史」；一张 card 内逐条 srow（标题 + 「来源：…」12 ink-3 + 状态 chip
// 有效 ok / 已撤销、已替代 neutral / 内容变更需复审 warn / 待恢复 warn + 匹配 chip）。行本身是展开按钮
// （aria-expanded），展开后才出正文、注意事项、停用理由与来源清单——原型的行只有一行高，治理细节不常驻。
// 曾经的写法：整理与限额用原生 <details> 三角 / <fieldset>，与 E 系列不符（票 16B 验收偏差），改为
// 本文件的 Disclosure 行 + 子件内 A9 字段。停用 / 恢复按钮保持原生 disabled（撤销理由未填时不可点，
// 测试与键盘行为依赖它；Button 组件只给 aria-disabled），只换外观。
const STATE_TONE = { active: 'ok', revoked: 'neutral', superseded: 'neutral', changed: 'warn', pending_recovery: 'warn' } as const
const nativeBtn = buttonVariants({ size: 'xs', variant: 'default' }) + ' disabled:cursor-default disabled:opacity-45'

type Entry = ExperienceEntryView['entry']

/** 行标题：正文首个非空行（原型标题就是经验本身的一句话）；空正文回落条目 id。 */
export function entryTitle(entry: Entry): string {
  return entry.body.split('\n').map((line) => line.replace(/^[#>*\-\s]+/, '').trim()).find(Boolean) ?? entry.entry_id
}

/** 行副标「来源：作者 · v3」：取首个来源作者，多来源时带总数；无来源写明无来源（不留空）。 */
export function entrySourceLine(entry: Entry, sourceCount: number, t: TFunction): string {
  const author = entry.sources[0]?.author
  if (!author) return t('experience.sourceNone', { version: entry.revision })
  return sourceCount > 1
    ? t('experience.sourceLineMore', { author, count: sourceCount, version: entry.revision })
    : t('experience.sourceLine', { author, version: entry.revision })
}

/** 折叠行（替代原生 details 三角）：按钮 aria-expanded + chevron，展开体在下方。 */
function Disclosure({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  return (
    <div data-experience-disclosure className="border-t border-line-soft first:border-t-0">
      <button type="button" aria-expanded={open} onClick={() => setOpen((v) => !v)}
        className="flex min-h-[40px] w-full items-center gap-2 px-4 text-left text-[13px] font-medium text-ink live:hover:bg-hover">
        <Icon name={open ? 'chevron-down' : 'chevron-right'} size={14} className="text-ink-3" />
        <span className="min-w-0 flex-1 truncate">{label}</span>
      </button>
      {open && <div className="px-4 pb-4">{children}</div>}
    </div>
  )
}

export function ExperienceEntries({ skill, onRecovered, selectedEntry, documentRevision = 0 }: { skill: string; onRecovered?: () => void; selectedEntry?: string; documentRevision?: number }) {
  const { t } = useTranslation()
  const [related, setRelated] = useState<{ skill: string; entryId: string } | null>(null)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')
  const [reasons, setReasons] = useState<Record<string, string>>({})
  const [revision, setRevision] = useState(0)
  const [open, setOpen] = useState<string | null>(selectedEntry ?? null)
  const [showHistory, setShowHistory] = useState(false)
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
  async function revoke(entry: Entry) {
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
  if (result.error) return <p role="alert" className="mt-3 text-[12.5px] text-red">{result.error}</p>
  const visible = result.entries.filter(({ entry }) => !selectedEntry || selectedEntry === entry.entry_id)
  // 「历史」看的是展开的那条；都没展开时看第一条（历史按条目分页，宿主没有跨条目的总历史）。
  const historyOf = visible.find(({ entry }) => entry.entry_id === open) ?? visible[0]
  const historyTip = visible.length === 0 ? t('experience.historyNone') : undefined
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
      {!selectedEntry && (
        <div className="mt-5 mb-2 flex items-center gap-2">
          <b className="min-w-0 flex-1 text-[12px] font-medium text-ink">{t('experience.entriesN', { count: visible.length })}</b>
          {result.entries.some((r) => r.recovery_pending) && <button type="button" className={nativeBtn} disabled={busy}
            title={`${t('experience.recover')} (${formatBinding(bindingFor('recoverExperience'))})`}
            onClick={() => void recover()}>{t('experience.recover')}</button>}
          <Button size="xs" variant="ghost" aria-pressed={showHistory} disabled={visible.length === 0} title={historyTip}
            className={showHistory ? 'bg-hover text-ink' : undefined} onClick={() => setShowHistory((v) => !v)}>
            <Icon name="clock" size={14} />{t('experience.historyShort')}
          </Button>
        </div>
      )}
      {notice && <p role="status" className="mb-2 whitespace-pre-wrap text-[12.5px] text-ink-2">{notice}</p>}
      {visible.length === 0 ? (
        <Empty size="sm" icon="book" title={t('experience.emptyEntries')} desc={t('experience.emptyEntriesBody')} />
      ) : (
        <div data-experience-card className="rounded-[var(--r-card)] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
          {visible.map(({ entry, state, match_reason, source_count }) => {
            const expanded = open === entry.entry_id
            return (
              <article key={entry.entry_id} data-experience-entry={entry.entry_id}
                className="border-t border-line-soft text-[13px] text-ink-2 first:border-t-0" onKeyDown={(e) => {
                  if (!e.repeat && matches(e.nativeEvent, bindingFor('revokeExperience')) && (state === 'active' || state === 'changed')) { e.preventDefault(); e.stopPropagation(); void revoke(entry) }
                }}>
                <button type="button" aria-expanded={expanded} onClick={() => setOpen(expanded ? null : entry.entry_id)}
                  className="flex min-h-[48px] w-full items-center gap-2 px-4 py-2 text-left live:hover:bg-hover">
                  <span className="min-w-0 flex-1">
                    <b className="block truncate text-[13px] font-medium text-ink">{entryTitle(entry)}</b>
                    <span className="block truncate text-[12px] text-ink-3">{entrySourceLine(entry, source_count, t)}</span>
                  </span>
                  <Badge variant={STATE_TONE[state]}>{t(`experience.${state}`)}</Badge>
                  <Badge>{t(`experience.${match_reason}`)}</Badge>
                </button>
                {expanded && (
                  <div data-experience-body className="px-4 pb-3">
                    <p className="whitespace-pre-wrap text-ink">{entry.body}</p>
                    {entry.notes && <p className="mt-1 text-[12px] text-ink-3">{entry.notes}</p>}
                    {(state === 'active' || state === 'changed') && <div className="mt-2 flex items-center gap-2">
                      <label className="flex min-w-0 flex-1 items-center gap-2 text-[12px] text-ink-3">{t('experience.revokeReason')}<input
                        className="h-7 min-w-0 flex-1 rounded-[var(--r-sm)] bg-field px-2 text-[12.5px] text-ink shadow-[var(--shadow-inset-field),inset_0_0_0_1px_var(--line)] outline-none focus:bg-surface focus:shadow-[var(--focus-ring)]" value={reasons[entry.entry_id] ?? ''} onChange={(e) => setReasons({ ...reasons, [entry.entry_id]: e.target.value })} /></label>
                      <button type="button" className={nativeBtn} disabled={busy || !reasons[entry.entry_id]?.trim()} onClick={() => void revoke(entry)}
                        title={`${t('experience.revoke')} (${formatBinding(bindingFor('revokeExperience'))})`}>{t('experience.revoke')}</button>
                    </div>}
                    <div className="mt-2 text-[12px] text-ink-3">{t('experience.version', { version: entry.revision })} · {t('experience.sourceCount', { count: source_count })}</div>
                    <ul className="text-[12px] text-ink-3">{entry.sources.slice(0, 20).map((source) => (
                      <li key={`${source.artifact_id}:${source.review_event}`}>
                        {source.author} · {source.artifact_id} · {t('experience.version', { version: source.artifact_version })}
                      </li>
                    ))}</ul>
                    {source_count > 20 && <p className="text-[12px] text-ink-3">{t('experience.sourcesOmitted', { count: source_count - 20 })}</p>}
                  </div>
                )}
              </article>
            )
          })}
        </div>
      )}
      {showHistory && historyOf && (
        <div data-experience-history className="mt-2 rounded-[var(--r-card)] bg-[var(--app-card)] px-4 py-3 shadow-[var(--shadow-card)]">
          <div className="mb-2 truncate text-[12px] font-medium text-ink">{t('experience.historyOf', { title: entryTitle(historyOf.entry) })}</div>
          <ExperienceHistory key={historyOf.entry.entry_id} projectRoot={result.projectRoot} skill={skill} entryId={historyOf.entry.entry_id}
            onRelated={(target, entryId) => setRelated({ skill: target, entryId })} />
        </div>
      )}
      {related && <aside aria-label={t('experience.related')} className="mt-2 rounded-[var(--r-card)] bg-inset px-4 py-3 shadow-[inset_0_0_0_1px_var(--line-soft)]">
        <strong className="text-[12px] font-medium text-ink">{related.skill} · {related.entryId}</strong>
        <ExperienceEntries key={`${related.skill}:${related.entryId}`} skill={related.skill} selectedEntry={related.entryId} />
      </aside>}
      {!selectedEntry && (
        <div data-experience-governance className="mt-3 rounded-[var(--r-card)] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
          <Disclosure label={t('experience.limits')}><ExperienceLimits onSaved={() => setRevision((n) => n + 1)} /></Disclosure>
          {result.document && <Disclosure label={t('experience.curateSection')}><ExperienceCuration document={result.document} /></Disclosure>}
        </div>
      )}
    </section>
  )
}
