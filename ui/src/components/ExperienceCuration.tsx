import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { ProjectSkillDocument } from '../gen/ProjectSkillDocument'

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
  return <details onKeyDown={(e) => {
    if (!e.repeat && matches(e.nativeEvent, bindingFor('curateLegacyExperience'))) { e.preventDefault(); e.stopPropagation(); void submit() }
  }}>
    <summary>{t('experience.curate')}</summary>
    <p>{t('experience.curationHelp')}</p>
    <textarea readOnly aria-label={t('experience.legacyOriginal')} value={document.content} onSelect={(e) => {
      const el = e.currentTarget
      if (el.selectionEnd <= el.selectionStart) return
      const start = document.content.lastIndexOf('\n', Math.max(0, el.selectionStart - 1)) + 1
      const next = document.content.indexOf('\n', el.selectionEnd - 1)
      const end = next < 0 ? document.content.length : next + 1
      setRange([start, end]); setBody(document.content.slice(start, end).trim())
    }} />
    <label>{t('experience.review')}<input type="number" min={1} step={1} value={review} onChange={(e) => setReview(e.target.value)} /></label>
    <label>{t('experience.curationBody')}<textarea value={body} onChange={(e) => setBody(e.target.value)} /></label>
    <label>{t('experience.curationNotes')}<textarea value={notes} onChange={(e) => setNotes(e.target.value)} /></label>
    <label>{t('experience.curationReason')}<input value={reason} onChange={(e) => setReason(e.target.value)} /></label>
    <label>{t('experience.roles')}<input value={roles} onChange={(e) => setRoles(e.target.value)} /></label>
    <label>{t('experience.stages')}<input value={stages} onChange={(e) => setStages(e.target.value)} /></label>
    <label>{t('experience.paths')}<input value={paths} onChange={(e) => setPaths(e.target.value)} /></label>
    <button type="button" disabled={!valid || busy} onClick={() => void submit()} title={`${t('experience.curate')} (${formatBinding(bindingFor('curateLegacyExperience'))})`}>{t('experience.curate')}</button>
    {notice && <p role="status">{notice}</p>}
  </details>
}
