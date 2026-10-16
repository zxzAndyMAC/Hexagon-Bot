import { useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { ProjectSkillDocument } from '../gen/ProjectSkillDocument'
import { buttonVariants } from '@/components/ui/button'
import { Field, Input, Textarea } from '@/components/ui/input'

// 功能缺口 D3（票 16B）：曾用原生 <details>/<summary> 三角自带折叠，与 E 系列不符；折叠改由 ExperienceEntries
// 的 Disclosure 行承担，本件只渲染表单（A9 字段）。提交按钮保持原生 disabled——测试与未选原文时不可点依赖它。

const nativeBtn = buttonVariants({ size: 'sm', variant: 'default' }) + ' disabled:cursor-default disabled:opacity-45'

export function ExperienceCuration({ document }: { document: ProjectSkillDocument }) {
  const { t } = useTranslation()
  const [range, setRange] = useState<[number, number] | null>(null)
  const [body, setBody] = useState('')
  const [notes, setNotes] = useState('')
  const [reason, setReason] = useState('')
  const [review, setReview] = useState('')
  const [roles, setRoles] = useState('')
  const [stages, setStages] = useState('')
  const [paths, setPaths] = useState('')
  const [busy, setBusy] = useState(false)
  const id = useId()
  const [notice, setNotice] = useState('')
  const valid = !!range && !!body.trim() && !!reason.trim() && Number.isSafeInteger(Number(review)) && Number(review) > 0
  async function submit() {
    if (!valid || !range || busy) return
    setBusy(true)
    try {
      const bytes = new TextEncoder()
      const digest = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes.encode(document.content.slice(...range))))).map((n) => n.toString(16).padStart(2, '0')).join('')
      const values = (text: string) => text.split(',').map((v) => v.trim()).filter(Boolean)
      const result = await api.curateLegacyExperience({ project_root: document.project_root, review_event: Number(review),
        legacy: { skill: document.skill, start_byte: bytes.encode(document.content.slice(0, range[0])).length, end_byte: bytes.encode(document.content.slice(0, range[1])).length, digest },
        request: { create_role_skill: false, body, notes, review_event: Number(review), conditions: { roles: values(roles), stages: values(stages), paths: values(paths) }, targets: [{ skill: document.skill, expected_digest: document.digest, reason }] } })
      setNotice(t('experience.curationSubmitted', { id: result.proposal_id }))
    } catch (e) { setNotice(errText(e)) }
    finally { setBusy(false) }
  }
  const f = (name: string) => `${id}-${name}`
  return <div data-experience-curation className="flex flex-col gap-3" onKeyDown={(e) => {
    if (!e.repeat && matches(e.nativeEvent, bindingFor('curateLegacyExperience'))) { e.preventDefault(); e.stopPropagation(); void submit() }
  }}>
    <p className="text-[12.5px] text-ink-3">{t('experience.curationHelp')}</p>
    <Field label={t('experience.legacyOriginal')} htmlFor={f('original')}>
      <Textarea id={f('original')} readOnly aria-label={t('experience.legacyOriginal')} value={document.content} className="max-h-[30vh] font-mono text-[12px]" onSelect={(e) => {
        const el = e.currentTarget
        if (el.selectionEnd <= el.selectionStart) return
        const start = document.content.lastIndexOf('\n', Math.max(0, el.selectionStart - 1)) + 1
        const next = document.content.indexOf('\n', el.selectionEnd - 1)
        const end = next < 0 ? document.content.length : next + 1
        setRange([start, end]); setBody(document.content.slice(start, end).trim())
      }} />
    </Field>
    <Field label={t('experience.review')} htmlFor={f('review')}>
      <Input id={f('review')} type="number" min={1} step={1} className="max-w-[160px] font-mono" value={review} onChange={(e) => setReview(e.target.value)} />
    </Field>
    <Field label={t('experience.curationBody')} htmlFor={f('body')}><Textarea id={f('body')} value={body} onChange={(e) => setBody(e.target.value)} /></Field>
    <Field label={t('experience.curationNotes')} htmlFor={f('notes')}><Textarea id={f('notes')} value={notes} onChange={(e) => setNotes(e.target.value)} /></Field>
    <Field label={t('experience.curationReason')} htmlFor={f('reason')}><Input id={f('reason')} value={reason} onChange={(e) => setReason(e.target.value)} /></Field>
    <div className="grid grid-cols-3 gap-3">
      <Field label={t('experience.roles')} htmlFor={f('roles')}><Input id={f('roles')} value={roles} onChange={(e) => setRoles(e.target.value)} /></Field>
      <Field label={t('experience.stages')} htmlFor={f('stages')}><Input id={f('stages')} value={stages} onChange={(e) => setStages(e.target.value)} /></Field>
      <Field label={t('experience.paths')} htmlFor={f('paths')}><Input id={f('paths')} value={paths} onChange={(e) => setPaths(e.target.value)} /></Field>
    </div>
    <div className="flex items-center gap-2">
      {notice ? <p role="status" className="min-w-0 flex-1 text-[12.5px] text-ink-2">{notice}</p> : <span className="flex-1" />}
      <button type="button" className={nativeBtn} disabled={!valid || busy} onClick={() => void submit()} title={`${t('experience.curate')} (${formatBinding(bindingFor('curateLegacyExperience'))})`}>{t('experience.curate')}</button>
    </div>
  </div>
}
