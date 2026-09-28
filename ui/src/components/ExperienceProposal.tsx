import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import type { ExperienceProposalView } from '../gen/ExperienceProposalView'

/** Governance 02: show the host-bound request, never infer authority from Markdown. */
export function ExperienceProposal({ proposalId }: { proposalId: string }) {
  const { t } = useTranslation()
  const [view, setView] = useState<ExperienceProposalView | null>(null)
  const [recovering, setRecovering] = useState(false)
  const [error, setError] = useState('')
  useEffect(() => {
    let current = true
    setView(null)
    setError('')
    api.experienceProposal(proposalId)
      .then((result) => { if (current) setView(result) })
      .catch((e) => { if (current) setError(errText(e)) })
    return () => { current = false }
  }, [proposalId])
  async function recover() {
    if (recovering) return
    setRecovering(true)
    try {
      const reports = await api.recoverExperience()
      const own = reports.find((r) => r.proposal_id === proposalId && r.state === 'conflict')
      setError(own ? t(`experience.${own.reason_code ?? 'recovery_failed'}`) : '')
      setView(await api.experienceProposal(proposalId))
      await useUiStore.getState().refresh()
    } catch (e) { setError(errText(e)) }
    finally { setRecovering(false) }
  }
  if (!view) return error ? <p role="alert">{error}</p> : null
  const { request, source } = view
  return (
    <section aria-label={t('experience.proposal')} onKeyDown={(e) => {
      if (view.recovery_pending && !e.repeat && matches(e.nativeEvent, bindingFor('recoverExperience'))) { e.preventDefault(); e.stopPropagation(); void recover() }
    }}>
      {error && <p role="alert">{error}</p>}
      {view.recovery_pending && <button type="button" disabled={recovering} onClick={() => void recover()}
        title={`${t('experience.recover')} (${formatBinding(bindingFor('recoverExperience'))})`}>{t('experience.recover')}</button>}
      <strong>{t('experience.proposal')}</strong>
      {request.create_role_skill && <p>{t('experience.roleCreation', { author: source.author })}</p>}
      {view.legacy && <p>{t('experience.historicalSource')} · {view.legacy.skill} · {view.legacy.start_byte}–{view.legacy.end_byte}</p>}
      {request.targets.length === 0 && <p role="status">{t('experience.targetRequired')}</p>}
      {view.previous_entries.map((entry) => <details key={`${entry.skill}:${entry.entry_id}`}><summary>{t('experience.previousVersion')} · {entry.entry_id} · {entry.revision}</summary><p>{entry.body}</p><p>{entry.notes}</p></details>)}
      <p style={{ whiteSpace: 'pre-wrap' }}>{request.body}</p>
      {request.notes && <p>{request.notes}</p>}
      <dl>
        <dt>{t('experience.source')}</dt>
        <dd>{source.author} · {source.artifact_id} · {t('experience.version', { version: source.artifact_version })}</dd>
        <dt>{t('experience.review')}</dt><dd>{source.review_event}</dd>
        <dt>{t('experience.roles')}</dt><dd>{request.conditions.roles.join(', ') || t('experience.unrestricted')}</dd>
        <dt>{t('experience.stages')}</dt><dd>{request.conditions.stages.join(', ') || t('experience.unrestricted')}</dd>
        <dt>{t('experience.paths')}</dt><dd>{request.conditions.paths.join(', ') || t('experience.unrestricted')}</dd>
      </dl>
      <ul>{request.targets.map((target) => (
        <li key={target.skill}>{target.skill} — {target.reason}
          {target.change && <p>{t(`experience.${target.change.kind}`)} · {target.change.entry_id} · {t('experience.version', { version: target.change.expected_revision })} · {target.change.reason}</p>}
        </li>
      ))}</ul>
    </section>
  )
}
