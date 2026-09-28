import { bindingFor, formatBinding, matches } from '../keymap'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ArtifactRow } from '../api'
import { useUiStore } from '../store'
import { CodeView, DiffView } from './DiffView'
import { diffLines } from '../diff'
import { langFor } from '../highlight'
import { Md } from './Md'

const chipCls = (s: string) =>
  s === 'stamped' ? 'amber' : s === 'valid' ? 'ok' : s === 'pending' ? 'warn' : ''

export function ArtifactTab({ path }: { path: string }) {
  const { t } = useTranslation()
  const artifacts = useUiStore((s) => s.artifacts)
  const team = useUiStore((s) => s.team)
  const { askConfirm, pushToast } = useUiStore()
  // ui-audit-2 票 08：本路径若有 active 提案 → 露「回滚提案」入口
  // （此前 rollbackProposal 命令死接线，cards.rollback 是死 i18n 键）。
  const rollbackButton = useRef<HTMLButtonElement>(null)
  const [experienceProposal, setExperienceProposal] = useState(false)
  const [policyRecovery, setPolicyRecovery] = useState(false)
  const [activeProposal, setActiveProposal] = useState<string | null>(null)
  useEffect(() => {
    let current = true
    api.proposals()
      .then(async (all) => {
        if (!current) return
        setPolicyRecovery(all.some((r) => r.artifact_path === path && r.recovery_pending))
        const hit = all.find((r) => r.status === 'active' && r.artifact_path === path)
        const experience = hit?.surface === 'skill' ? await api.experienceProposal(hit.id) : null
        if (!current) return
        setExperienceProposal(!!experience)
        setActiveProposal(hit?.id ?? null)
      })
      .catch(() => { if (current) { setActiveProposal(null); setPolicyRecovery(false); setExperienceProposal(false) } })
    return () => { current = false }
  }, [path, artifacts])
  // 同路径版本链（升序）
  const versions = useMemo(
    () => artifacts.filter((a) => a.path === path).sort((a, b) => a.version - b.version),
    [artifacts, path],
  )
  const latest = versions[versions.length - 1]
  const [vA, setVA] = useState<number | null>(null) // 对比左
  const [vB, setVB] = useState<number | null>(null) // 查看/对比右
  const [mode, setMode] = useState<'content' | 'diff'>('content')
  const [textA, setTextA] = useState<string | null>(null)
  const [textB, setTextB] = useState<string | null>(null)

  const isMd = path.toLowerCase().endsWith('.md')
  const [mdView, setMdView] = useState<'preview' | 'source'>('preview')

  const selB = vB ?? latest?.version ?? 1
  const selA = vA ?? (versions.length > 1 ? versions[versions.length - 2]?.version ?? 1 : 1)

  const selected = versions.find((a) => a.version === selB)
  const review = selected?.review

  const currentWorktree = mode === 'content' && selB === latest?.version
  useEffect(() => {
    let live = true
    // Reliability 16: current delivery is the worktree, historical versions are
    // immutable evidence. A registered snapshot cannot hide an external edit.
    const read = currentWorktree ? api.artifactContent(path) : api.artifactContentAt(path, selB)
    read.then((c) => { if (live) setTextB(c) }).catch(() => { if (live) setTextB(null) })
    return () => { live = false }
  }, [path, selB, currentWorktree, artifacts])
  useEffect(() => {
    if (mode !== 'diff') return
    let live = true
    api.artifactContentAt(path, selA).then((c) => { if (live) setTextA(c) })
    return () => { live = false }
  }, [path, selA, mode])

  const ops = useMemo(
    () => (mode === 'diff' && textA != null && textB != null
      ? diffLines(textA.split('\n'), textB.split('\n'))
      : []),
    [mode, textA, textB],
  )

  return (
    <div onKeyDown={(e) => {
      const state = useUiStore.getState()
      if (experienceProposal && !e.repeat && state.modalScope === 'workbench' && !state.confirmReq && matches(e.nativeEvent, bindingFor('rollbackExperience'))) { e.preventDefault(); e.stopPropagation(); rollbackButton.current?.click() }
    }} style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      <div className="row-line" style={{ padding: '8px 14px', display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
        <span className="mono" style={{ fontWeight: 560, fontSize: 13 }}>{path}</span>
        {latest && <span className="chip">{latest.kind}</span>}
        {latest && <span className={`chip ${latest.materialization === 'pending_recovery' ? 'warn' : chipCls(latest.status)}`}>{latest.materialization === 'pending_recovery' ? t('side.pendingRecovery') : t(`side.${latest.status}`, latest.status)}</span>}
        {latest?.author && (
          <span className="dim3" style={{ fontSize: 11 }}>
            {team.find((m) => m.id === latest.author)?.role ?? latest.author}
          </span>
        )}
        <div style={{ flex: 1 }} />
        {policyRecovery && <span role="alert" className="chip warn">{t('policy.recovery')}</span>}
        {activeProposal && (
          <button
            ref={rollbackButton}
            className="btn danger"
            style={{ fontSize: 11, padding: '2px 8px' }}
            disabled={policyRecovery}
            title={experienceProposal ? `${t('experience.withdraw')} (${formatBinding(bindingFor('rollbackExperience'))})` : t('cards.rollbackHint')}
            onClick={() =>
              askConfirm({
                title: t('cards.rollbackTitle', { id: activeProposal }),
                body: t(experienceProposal ? 'experience.withdrawBody' : 'cards.rollbackBody'),
                danger: true,
                confirmLabel: t('cards.rollback'),
                run: () =>
                  api
                    .rollbackProposal(activeProposal)
                    .then(() => setActiveProposal(null))
                    .catch((e) => pushToast(errText(e), 'err')),
              })
            }
          >
            {t(experienceProposal ? 'experience.withdraw' : 'cards.rollback')}
          </button>
        )}
        {isMd && mode === 'content' && (
          <>
            <button
              className={`btn ${mdView === 'preview' ? 'primary' : ''}`}
              style={{ fontSize: 11, padding: '2px 8px' }}
              onClick={() => setMdView('preview')}
            >
              {t('art.preview')}
            </button>
            <button
              className={`btn ${mdView === 'source' ? 'primary' : ''}`}
              style={{ fontSize: 11, padding: '2px 8px' }}
              onClick={() => setMdView('source')}
            >
              {t('art.source')}
            </button>
          </>
        )}
        {versions.length > 1 && (
          <>
            <button
              className={`btn ${mode === 'content' ? 'primary' : ''}`}
              style={{ fontSize: 11, padding: '2px 8px' }}
              onClick={() => setMode('content')}
            >
              {t('art.content')}
            </button>
            <button
              className={`btn ${mode === 'diff' ? 'primary' : ''}`}
              style={{ fontSize: 11, padding: '2px 8px' }}
              onClick={() => setMode('diff')}
            >
              {t('art.compare')}
            </button>
          </>
        )}
      </div>
      <div style={{ padding: '6px 14px', display: 'flex', gap: 10, alignItems: 'center', borderBottom: '1px solid var(--border)' }}>
        {mode === 'diff' && (
          <>
            <VersionPicker versions={versions} value={selA} onChange={setVA} />
            <span className="dim3">{t('art.vs')}</span>
          </>
        )}
        <VersionPicker versions={versions} value={selB} onChange={setVB} />
        {currentWorktree && <span className="dim3">{t('art.currentWorktree')}</span>}
      </div>
      {review && <div style={{ padding: '8px 14px', display: 'flex', gap: 8 }}>
        <span className={`chip ${review.status === 'passed' ? 'ok' : 'warn'}`}>{t(review.status === 'passed' ? 'art.reviewPassed' : review.status === 'rejected' ? 'art.reviewRejected' : 'art.reviewStale')}</span>
        <span className="dim3">{review.reviewer}{review.version != null ? ` · v${review.version}` : ''}</span>
      </div>}
      {latest?.materialization === 'pending_recovery' && <div className="dim3" style={{ padding: '8px 14px' }}>{t('art.pendingRecovery')}</div>}
      {mode === 'content'
        ? (textB != null
            ? (isMd && mdView === 'preview'
                ? (
                  <div className="msg-body" style={{ flex: 1, overflowY: 'auto', maxWidth: 'none', maxHeight: 'none', margin: 14 }}>
                    <Md>{textB}</Md>
                  </div>
                )
                : <CodeView text={textB} lang={langFor(path)} />)
            : <div className="dim3" style={{ padding: 14 }}>{t('art.unavailable')}</div>)
        : (textA != null && textB != null ? <DiffView ops={ops} /> : <div className="dim3" style={{ padding: 14 }}>{t('art.unavailable')}</div>)}
    </div>
  )
}

function VersionPicker({ versions, value, onChange }: {
  versions: ArtifactRow[]
  value: number
  onChange: (v: number) => void
}) {
  const { t } = useTranslation()
  return (
    <div style={{ display: 'flex', gap: 4 }}>
      {versions.map((a) => (
        <button
          key={a.id}
          className={`chip ${a.version === value ? 'amber' : ''}`}
          style={{ cursor: 'pointer' }}
          onClick={() => onChange(a.version)}
        >
          {t('art.version', { n: a.version })}
        </button>
      ))}
    </div>
  )
}
