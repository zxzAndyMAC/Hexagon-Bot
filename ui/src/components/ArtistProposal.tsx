// 头像提案卡（owner-followups-1015 票 20，规范 components D12「头像提案卡」补设计，CONTEXT.md「画师」）。
// core `artist_proposal::PROPOSAL` System 事件的渲染：画师发言（C1 头行，同 ArtistNotice）+ D12 形卡——
// 每格角色名 + 生成图，「换一张 / 跳过」；底部「全部应用」；提示词折叠。
// 不是待决卡（2026-10-17 负责人裁决）：不进待决坞、托管不处理、不吃裁决键——这里不注册任何 keymap
// 动作，也不经 settleDecision；只有负责人点「全部应用」才按角色写头像（core 走票 17 写入路径）。
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type TimelineItem } from '../api'
import type { ArtistProposal as Proposal } from '../gen/ArtistProposal'
import { useUiStore } from '../store'
import { fmtTime } from '../usage'
import { DecisionButton, DecisionShell } from './DecisionShell'
import { Icon } from './Icon'
import { Avatar as Face } from './ui/avatar'
import './ArtistProposal.css'

export const ARTIST_PROPOSAL_KIND = 'artist_avatar_proposal'

export function ArtistProposalCard({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const p = item.event.payload
  const id = String(p.proposal_id ?? '')
  const speaker = String(p.speaker ?? '')
  const time = fmtTime(item.event.created_at)
  const invalidate = useUiStore((s) => s.invalidate)
  const [proposal, setProposal] = useState<Proposal | null>(null)
  const [error, setError] = useState('')
  const [skipped, setSkipped] = useState<ReadonlySet<string>>(new Set())
  const [redrawing, setRedrawing] = useState<string | null>(null)
  const [applying, setApplying] = useState(false)

  const load = useCallback(() => {
    let live = true
    api.artistProposal(id).then((v) => { if (live) setProposal(v) })
      .catch((e: unknown) => { if (live) setError(errText(e)) })
    return () => { live = false }
  }, [id])
  useEffect(load, [load])

  const applied = (proposal?.applied.length ?? 0) > 0
  const picked = (proposal?.items ?? []).filter((it) => it.image && !skipped.has(it.role)).map((it) => it.role)
  const busy = applying || redrawing !== null

  const toggleSkip = (role: string) => setSkipped((s) => {
    const next = new Set(s)
    if (next.has(role)) next.delete(role)
    else next.add(role)
    return next
  })
  const redraw = async (role: string) => {
    setRedrawing(role)
    setError('')
    try { setProposal(await api.artistRegenerate(id, role)) }
    catch (e) { setError(errText(e)) }
    finally { setRedrawing(null) }
  }
  const apply = async () => {
    setApplying(true)
    setError('')
    try {
      setProposal(await api.artistApply(id, picked))
      await invalidate('team')
    } catch (e) { setError(errText(e)) }
    finally { setApplying(false) }
  }

  return (
    <div className="tl-msg" data-artist-proposal={id}>
      <Face agentId={String(p.speaker_code ?? speaker)} name={speaker} size={30} />
      <div className="tl-grow">
        <div className="tl-who">
          <b>{speaker}</b>
          {time && <span className="t">{time}</span>}
        </div>
        <DecisionShell cat="info" icon="palette" title={t('artist.proposalTitle')}
          sub={t('artist.proposalSub', { count: proposal?.items.length ?? 0, style: proposal?.style ?? String(p.style ?? '') })}
          note={applied ? t('artist.applied', { count: proposal!.applied.length }) : t('artist.applyNote')}
          actions={proposal && !applied && (
            <DecisionButton variant="primary" busy={applying} disabled={picked.length === 0 || redrawing !== null}
              onClick={() => void apply()}>{t('artist.applyAll')}</DecisionButton>
          )}>
          {error && <p role="alert" className="artist-error">{error}</p>}
          {!proposal ? (!error && <p role="status">{t('artist.loading')}</p>) : <>
            <div className="artist-cells">
              {proposal.items.map((it) => {
                const skip = skipped.has(it.role)
                const done = proposal.applied.includes(it.role)
                return (
                  <article key={it.role} className="artist-cell" data-role={it.role}
                    data-skipped={skip || undefined} data-applied={done || undefined} aria-busy={redrawing === it.role || undefined}>
                    <figure>
                      {it.image
                        ? <img src={it.image} alt={t('artist.avatarOf', { role: it.role })} />
                        : <span className="artist-missing"><Icon name="image" size={18} />{t('artist.failed')}</span>}
                      {done && <span className="chip info artist-chip">{t('artist.appliedChip')}</span>}
                    </figure>
                    <b>{it.role}</b>
                    {!applied && (
                      <div className="artist-cell-actions">
                        <DecisionButton variant="ghost" busy={redrawing === it.role} disabled={busy}
                          onClick={() => void redraw(it.role)}>{t('artist.redraw')}</DecisionButton>
                        <DecisionButton variant="ghost" disabled={busy || !it.image} aria-pressed={skip}
                          onClick={() => toggleSkip(it.role)}>{skip ? t('artist.unskip') : t('artist.skip')}</DecisionButton>
                      </div>
                    )}
                  </article>
                )
              })}
            </div>
            <details className="artist-prompts">
              <summary>{t('artist.prompts')}</summary>
              <dl>{proposal.items.map((it) => (
                <div key={it.role}><dt>{it.role}</dt><dd>{it.prompt}</dd></div>
              ))}</dl>
            </details>
          </>}
        </DecisionShell>
      </div>
    </div>
  )
}
