import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import i18n from '../i18n'
import { api, errText, type PendingQuestion } from '../api'
import { useUiStore } from '../store'
import { extractDiffBlock, parseUnifiedDiff, type DiffOp } from '../diff'
import { DiffView } from './DiffView'
import { bindingFor, formatBinding, matches } from '../keymap'
import { kindTitleKey, policyNeedsQuality, rejectReasonWithJudge, severityOf } from '../decisions'
import { Icon, type IconName } from './Icon'
import { StageEvidence } from './StageEvidence'
import type { StageEvidence as Evidence } from '../gen/StageEvidence'

type TFn = (key: string, opts?: Record<string, unknown>) => string

function judgeRationale(p: Record<string, unknown>, t: TFn): string {
  const reason = p.judge_reason
  if (reason && typeof reason === 'object') {
    const code = String((reason as { code?: unknown }).code ?? '')
    if (code) return t(`cards.reason_${code}`, reason as Record<string, unknown>)
  }
  return String(p.judge_advice ?? '')
}

function ProvenanceLines({ payload }: { payload: Record<string, unknown> }) {
  const { t } = useTranslation()
  const fact = payload.provenance
  const remote = payload.remote_delta
  if (!fact && !remote) return null
  const lines: string[] = []
  if (fact && typeof fact === 'object') {
    const f = fact as { path?: unknown; downloaded?: unknown; steps_ago?: unknown }
    const n = Number(f.steps_ago ?? 0)
    const when = n === 0 ? t('cards.justNow') : n === 1 ? t('cards.oneStepAgo') : t('cards.stepsAgo', { n })
    const key = f.downloaded ? 'cards.provenanceDownloaded' : 'cards.provenanceCreated'
    lines.push(t(key, { path: String(f.path ?? ''), when }))
  }
  if (remote && typeof remote === 'object') {
    const name = String((remote as { remote?: unknown }).remote ?? '')
    if (name) lines.push(t('cards.remoteUnknown', { remote: name }))
  }
  if (!lines.length) return null
  return (
    <div className="dim3" style={{ fontSize: 11, margin: '0 0 8px' }}>
      {lines.map((line) => <div key={line}>{line}</div>)}
    </div>
  )
}

function CardShell({ tone, icon, title, children }: {
  tone: 'ask' | 'stamp' | 'flag' | 'danger'
  icon: IconName
  title: React.ReactNode
  children: React.ReactNode
}) {
  const cls = { ask: 'card-ask', stamp: 'card-stamp', flag: 'card-flag', danger: 'card-danger' }[tone]
  const color = { ask: 'var(--accent)', stamp: 'var(--accent)', flag: 'var(--flag)', danger: 'var(--err)' }[tone]
  return (
    <div className={cls} style={{ padding: '10px 14px', margin: '6px 14px 0', ...(tone === 'danger' ? { borderColor: 'var(--err)' } : {}) }}>
      <div style={{ fontWeight: 510, color, marginBottom: 4, display: 'flex', alignItems: 'center', gap: 6 }}>
        <Icon name={icon} size={13} /> {title}
      </div>
      {children}
    </div>
  )
}

// 票 19（方向卡 4）：提案卡内嵌 diff 手风琴——chevron 展开懒跑
// proposals→artifactContent→extractDiffBlock 链；限高 200px 内滚 +
// 「打开完整对照」tab 链接。加载失败回退开 tab 路径 + toast（票 04 约定）。
function InlineDiff({ proposalId }: { proposalId: string }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const pushToast = useUiStore((s) => s.pushToast)
  const [open, setOpen] = useState(false)
  const [loading, setLoading] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [ops, setOps] = useState<DiffOp[] | null>(null)

  const fetchDiff = async (): Promise<{ diff: string | null; artPath: string | null }> => {
    const props = await api.proposals()
    const pr = props.find((x) => String(x.id) === proposalId)
    const ap = pr?.artifact_path ? String(pr.artifact_path) : null
    if (!ap) return { diff: null, artPath: null }
    const body = await api.artifactContent(ap)
    return { diff: extractDiffBlock(body), artPath: ap }
  }

  const openFull = async () => {
    try {
      const { diff, artPath: ap } = await fetchDiff()
      if (diff) {
        openTab({ id: `patch:${proposalId}`, kind: 'diff', title: `${proposalId} diff`, patchText: diff })
      } else if (ap) {
        openTab({ id: `art:${ap}`, kind: 'artifact', title: ap, path: ap })
      }
    } catch (e) {
      pushToast(errText(e), 'err')
    }
  }

  const toggle = async () => {
    if (open) { setOpen(false); return }
    setOpen(true)
    if (loaded || loading) return
    setLoading(true)
    try {
      const { diff } = await fetchDiff()
      setOps(diff ? parseUnifiedDiff(diff) : null)
      setLoaded(true)
    } catch (e) {
      setOpen(false)
      pushToast(errText(e), 'err')
      void openFull() // 回退开 tab 路径（其内部再 toast）
    } finally {
      setLoading(false)
    }
  }

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 6 }}>
        <button
          className="icon-btn"
          style={{ display: 'inline-flex', alignItems: 'center', gap: 4, fontSize: 11, color: 'var(--accent)' }}
          title={t('cards.inlineDiff')}
          onClick={() => void toggle()}
        >
          <Icon name={open ? 'chevron-down' : 'chevron-right'} size={10} /> {t('cards.inlineDiff')}
        </button>
        {loaded && (
          <button
            className="btn"
            style={{ fontSize: 10, padding: '1px 8px' }}
            onClick={() => void openFull()}
          >
            {t('cards.openFullDiff')}
          </button>
        )}
      </div>
      {open && (
        <div style={{ maxHeight: 200, overflowY: 'auto', marginTop: 4, border: '1px solid var(--border)', borderRadius: 6, display: 'flex', minHeight: 0 }}>
          {loading
            ? <div className="dim3" style={{ fontSize: 11, padding: 8 }}>…</div>
            : ops
              ? <DiffView ops={ops} />
              : <div className="dim3" style={{ fontSize: 11, padding: 8 }}>{t('cards.noDiff')}</div>}
        </div>
      )}
    </div>
  )
}

function Btn({ onClick, primary, danger, title, disabled, children }: {
  onClick: () => Promise<unknown>; primary?: boolean; danger?: boolean; title?: string; disabled?: boolean; children: React.ReactNode
}) {
  const [busy, setBusy] = useState(false)
  const invalidate = useUiStore((s) => s.invalidate)
  const pushToast = useUiStore((s) => s.pushToast)
  return (
    <button
      className={`btn ${primary ? 'primary' : ''} ${danger ? 'danger' : ''}`}
      title={title}
      disabled={busy || disabled}
      onClick={async () => {
        setBusy(true)
        // ui-audit 票 04（P1-6）：待决按钮统一错误出口——失败 toast，
        // busy 复位、卡保留原地（后端幂等，重试安全）。
        try { await onClick(); await invalidate() }
        catch (e) { pushToast(i18n.t('errors.actionFailed', { detail: errText(e) }), 'err') }
        finally { setBusy(false) }
      }}
    >
      {children}
    </button>
  )
}

// beautiful-ui 移植票 01（spec D2/D9）：待决卡进出场是本批唯一 motion 用点。
// enter=落下淡入；exit=淡出+高度收拢，剩余卡经 layout 上移补位。
// 逐帧编排一律走 index.css keyframes，不在此扩散 motion 用途。
function MotionCard({ children, itemRef, current }: {
  children: React.ReactNode
  itemRef?: React.Ref<HTMLDivElement>
  current?: boolean
}) {
  return (
    <motion.div
      layout
      ref={itemRef}
      aria-current={current ? 'true' : undefined}
      initial={{ opacity: 0, y: -8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, overflow: 'hidden' }}
      transition={{ duration: 0.18, ease: [0.23, 1, 0.32, 1] }}
    >
      {children}
    </motion.div>
  )
}

function ActionRecovery({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const [reason, setReason] = useState('')
  const [acceptsDuplicate, setAcceptsDuplicate] = useState(false)
  const container = useRef<HTMLDivElement>(null)
  const id = String(q.payload.action_id)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!top || e.repeat || e.defaultPrevented) return
      const index = (['reconcileAction', 'abandonAction', 'retryAction'] as const).findIndex((action) => matches(e, bindingFor(action)))
      if (index < 0) return
      e.preventDefault()
      if (useUiStore.getState().modalScope !== 'workbench') {
        useUiStore.getState().pushToast(i18n.t('decisions.scopeBlocked'))
        return
      }
      container.current?.querySelectorAll('button')[index]?.click()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [top])
  return <CardShell tone="ask" icon="warn" title={t('cards.actionUnknown')}>
    <div ref={container}>
      <div className="dim3">{t('cards.actionUnknownHint')}</div>
      <code>{id}</code>
      {q.payload.reconciliation_evidence != null && <p>{String(q.payload.reconciliation_evidence)}</p>}
      <label>{t('cards.actionReason')}<input value={reason} onChange={(e) => setReason(e.target.value)} /></label>
      <label><input type="checkbox" checked={acceptsDuplicate} onChange={(e) => setAcceptsDuplicate(e.target.checked)} />{t('cards.duplicateRisk')}</label>
      <div style={{ display: 'flex', gap: 8 }}>
        <Btn title={formatBinding(bindingFor('reconcileAction'))} onClick={() => api.reconcileToolAction(id)}>{t('cards.reconcileAction')}</Btn>
        <Btn danger disabled={!reason.trim()} title={formatBinding(bindingFor('abandonAction'))} onClick={() => api.abandonToolAction(id, reason)}>{t('cards.abandonAction')}</Btn>
        <Btn danger disabled={!reason.trim() || !acceptsDuplicate} title={formatBinding(bindingFor('retryAction'))} onClick={() => api.retryToolAction(id, reason, acceptsDuplicate)}>{t('cards.retryAction')}</Btn>
      </div>
    </div>
  </CardShell>
}

function PolicyReport({ q }: { q: PendingQuestion }) {
  const { t } = useTranslation()
  const openButton = useRef<HTMLButtonElement>(null)
  const report = q.payload.evidence as Record<string, unknown> | null | undefined
  return <div onKeyDown={(e) => {
    if (!matches(e, bindingFor('openPolicyReport'))) return
    e.preventDefault()
    if (!e.repeat && useUiStore.getState().modalScope === 'workbench') openButton.current?.click()
  }}>
    <p className="dim">{t('policy.ownerOnly')}</p>
    {policyNeedsQuality(q) && <p role="status">{t('policy.unverified')}</p>}
    {q.payload.policy_recovery === true && <p role="alert">{t('policy.recovery')}</p>}
    {report && <p className="mono">{t('policy.scores', { baseline: report.baseline_score ?? '—', candidate: report.candidate_score ?? '—' })}</p>}
    <button ref={openButton} className="btn" title={`${t('policy.report')} · ${formatBinding(bindingFor('openPolicyReport'))}`} onClick={async () => {
      try {
        const proposal = (await api.proposals()).find((p) => p.id === q.payload.proposal_id)
        if (!proposal?.artifact_path) throw new Error(t('errors.not_found'))
        useUiStore.getState().openTab({ id: `art:${proposal.artifact_path}`, kind: 'artifact', title: proposal.artifact_path, path: proposal.artifact_path })
        useUiStore.getState().closePendingDialog()
      } catch (e) { useUiStore.getState().pushToast(errText(e), 'err') }
    }}>{t('policy.report')}</button>
  </div>
}

function AcceptanceException({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const [evidence, setEvidence] = useState<Evidence | null>(null)
  const [reason, setReason] = useState('')
  const [selected, setSelected] = useState<string[]>([])
  const [error, setError] = useState('')
  const container = useRef<HTMLDivElement>(null)
  const revision = useUiStore((s) => s.evidenceRevision)
  const event = useUiStore((s) => s.timeline.at(-1)?.event.id)
  useEffect(() => {
    let live = true
    let request = 0
    const refresh = () => {
      const token = ++request
      setEvidence(null)
      setSelected([])
      void api.stageEvidence().then((next) => { if (live && token === request) { setEvidence(next); setError('') } },
        (e) => { if (live && token === request) setError(errText(e)) })
    }
    refresh()
    window.addEventListener('focus', refresh)
    return () => { live = false; window.removeEventListener('focus', refresh) }
  }, [q.id, revision, event])
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (!top || e.repeat || e.defaultPrevented || !matches(e, bindingFor('acceptException'))) return
      e.preventDefault()
      if (useUiStore.getState().modalScope !== 'workbench') {
        useUiStore.getState().pushToast(i18n.t('decisions.scopeBlocked'))
        return
      }
      container.current?.querySelector<HTMLButtonElement>('button.primary')?.click()
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [top])
  const current = evidence?.run_id === q.payload.run_id && evidence?.fingerprint === q.payload.fingerprint
  const candidates = evidence?.exceptions.filter((item) => !item.accepted) ?? []
  const items = candidates.filter((item) => selected.includes(JSON.stringify(item.requirement)))
  return <CardShell tone="ask" icon="warn" title={t('exceptions.title')}>
    <div ref={container}>
      <p className="dim">{t('exceptions.hint')}</p>
      {error && <div role="alert">{error}</div>}
      {evidence && !current && <p role="alert">{t('exceptions.stale')}</p>}
      {current && candidates.map((item) => {
        const id = JSON.stringify(item.requirement)
        return <label key={id} style={{ display: 'block' }}>
          <input type="checkbox" checked={selected.includes(id)} onChange={(e) => setSelected((old) => e.target.checked ? [...old, id] : old.filter((value) => value !== id))} />{item.label}
        </label>
      })}
      <label>{t('exceptions.reason')}<textarea value={reason} onChange={(e) => setReason(e.target.value)} /></label>
      <div style={{ display: 'flex', gap: 8 }}>
        <Btn primary title={formatBinding(bindingFor('acceptException'))}
          disabled={!current || !reason.trim() || items.length === 0 || evidence?.missing.some((m) => m.startsWith('action:'))}
          onClick={() => api.acceptDeliveryException(q.id, String(q.payload.fingerprint), items.map((item) => item.requirement), reason)}>{t('exceptions.accept')}</Btn>
        <Btn title={formatBinding(bindingFor('reject'))} onClick={() => api.cancelAcceptanceException(q.id)}>{t('exceptions.cancel')}</Btn>
      </div>
    </div>
  </CardShell>
}

export function PendingCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const [shape, setShape] = useState('')
  const [rejectReason, setRejectReason] = useState('')
  const rewindStageRef = useRef<HTMLSelectElement>(null)
  const rewindStageInputRef = useRef<HTMLInputElement>(null)
  const revisionNoteRef = useRef<HTMLInputElement>(null)
  const stageNames = useUiStore((s) => s.stages)
  const p = q.payload
  const approveTip = formatBinding(bindingFor('approve'))
  const rejectTip = formatBinding(bindingFor('reject'))

  if (q.kind === 'permission') {
    const tool = String(p.tool ?? '')
    const input = p.input as Record<string, unknown> | undefined
    const summary = input ? Object.entries(input).map(([k, v]) => `${k}=${typeof v === 'string' ? v.slice(0, 60) : JSON.stringify(v)}`).join(' ') : ''
    const safety = Boolean(p.safety_net)
    return (
      <CardShell tone="ask" icon="warn" title={t('cards.ask')}>
        <div className="mono" style={{ fontSize: 12 }}>
          <strong>{tool}</strong> <span className="dim">{summary}</span>
        </div>
        <div className="dim3" style={{ fontSize: 11, margin: '4px 0 8px' }}>
          {String(p.reason ?? '')}
          {safety && <span className="chip err" style={{ marginLeft: 8 }}>{t('cards.askSafetyNet')}</span>}
        </div>
        {Array.isArray(p.write_targets) && p.write_targets.length > 0 && <div className="dim3" style={{ marginBottom: 8 }}>
          {t('cards.writeTargets')} <span className="mono">{p.write_targets.filter((v): v is string => typeof v === 'string').join(' · ')}</span>
        </div>}
        <ProvenanceLines payload={p} />
        {!safety && (
          <div style={{ display: 'flex', gap: 6, marginBottom: 8 }}>
            <input
              value={shape}
              onChange={(e) => setShape(e.target.value)}
              placeholder={t('cards.shapeHint')}
              className="mono"
              style={{ flex: 1, fontSize: 11, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px', outline: 'none' }}
            />
            <Btn onClick={() => api.answerPermission(q.id, true, shape || undefined)}>{t('cards.remember')}</Btn>
          </div>
        )}
        <div style={{ display: 'flex', gap: 8 }}>
          <Btn primary onClick={() => api.answerPermission(q.id, true)}>{t('cards.allowOnce')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.answerPermission(q.id, false)}>{t('cards.deny')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'stamp' && p.sub === 'acceptance_exception') return <AcceptanceException q={q} top={top} />

  if (q.kind === 'stamp' && !p.proposal_id) {
    const finalGate = p.final_acceptance === true
    const names = [...new Set(stageNames.map((s) => s.stage))]
    return (
      <CardShell tone="stamp" icon="stamp" title={`${t('cards.stageStamp')} · ${String(p.stage ?? '')}`}>
        <StageArtifacts runId={String(p.run_id ?? '')} />
        <StageEvidence runId={String(p.run_id ?? '')} />
        {finalGate && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6, marginTop: 8 }}>
            {names.length > 0 ? (
              <select
                ref={rewindStageRef}
                aria-label={t('cards.finalStagePh')}
                defaultValue=""
                style={{ fontSize: 12, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px' }}
              >
                <option value="">{t('cards.finalStagePh')}</option>
                {names.map((n) => <option key={n} value={n}>{n}</option>)}
              </select>
            ) : (
              <input
                ref={rewindStageInputRef}
                aria-label={t('cards.finalStagePh')}
                placeholder={t('cards.finalStagePh')}
                style={{ fontSize: 12, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px' }}
              />
            )}
            <input
              ref={revisionNoteRef}
              aria-label={t('cards.finalNotePh')}
              placeholder={t('cards.finalNotePh')}
              style={{ fontSize: 12, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px' }}
            />
          </div>
        )}
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <Btn primary onClick={() => api.stamp()}>{t('cards.confirm')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => (finalGate
            ? api.rejectStamp(
                rewindStageRef.current?.value ?? rewindStageInputRef.current?.value ?? '',
                revisionNoteRef.current?.value ?? '',
              )
            : api.rejectStamp())}>{t('cards.reject')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'stamp' && p.proposal_id) {
    const policy = p.surface === 'pack_copy'
    const warnings = (p.warnings as string[] | undefined) ?? []
    return (
      <CardShell tone="stamp" icon="hex" title={`${t(policy ? 'policy.title' : 'cards.proposalStamp')} · ${String(p.proposal_id)}`}>
        <div className="dim" style={{ fontSize: 12 }}>
          {t('cards.proposal')} · {t(`cards.surface_${String(p.surface)}`, { defaultValue: String(p.surface ?? '') })}
        </div>
        {policy && <PolicyReport q={q} />}
        {warnings.length > 0 && (
          <div className="accent" style={{ fontSize: 12, margin: '4px 0', display: 'flex', alignItems: 'center', gap: 5 }}>
            <Icon name="warn" size={11} /> {warnings.length
              ? warnings.map((w) => t(`cards.warn_${w}`, { defaultValue: w })).join(' · ')
              : String(p.warning_text ?? '')}
          </div>
        )}
        {/* 票 09：judge 建议行——闭集 chip + 大白话行（i18n 模板,
            rationale 是生成内容按数据展示）。判定是建议不是授权。
            票 07（P2-15）：视觉降权——stamp 不再 ok 绿、reject 不再 err 红
            （err 留给确定的危险；judge 只是机器建议）。 */}
        {p.judge_verdict != null && (() => {
          const vk = String(p.judge_verdict)
          const suffix = vk === 'stamp' ? 'Stamp' : vk === 'reject' ? 'Reject' : 'NeedsHuman'
          return (
            <div style={{ fontSize: 11, margin: '4px 0', display: 'flex', alignItems: 'baseline', gap: 6 }}>
              <span className="dim3" style={{ fontSize: 10 }}>{t('cards.judgeAdvice')}</span>
              <span className={`chip ${vk === 'stamp' ? '' : 'warn'}`} style={{ fontSize: 10 }}>
                {t(`cards.judge${suffix}`)}
              </span>
              <span className="dim3">{t(`cards.judgeLine${suffix}`, { rationale: judgeRationale(p, t) })}</span>
              {p.judge_backend != null && <span className="dim3" style={{ fontSize: 10 }}>{String(p.judge_backend)}</span>}
            </div>
          )
        })()}
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          {(() => {
            // 票 07（P2-15）：逆建议留痕——judge=stamp 时驳回、judge=reject 时确认，
            // 按钮挂 againstJudge 小标记；驳回 reason 追加 judge=<verdict> 对照。
            const jv = p.judge_verdict != null ? String(p.judge_verdict) : null
            const mk = (bad: boolean) =>
              bad && <span className="dim3" style={{ fontSize: 9, fontWeight: 400 }}> {t('cards.againstJudge')}</span>
            return (
              <>
                <Btn primary disabled={p.policy_recovery === true || policyNeedsQuality(q)} title={approveTip} onClick={() => api.confirmProposal(q.id)}>
                  {t(policy ? 'policy.adopt' : 'cards.confirm')}{top && ` ${approveTip}`}{mk(jv === 'reject')}
                </Btn>
                <input
                  value={rejectReason}
                  onChange={(e) => setRejectReason(e.target.value)}
                  placeholder={t('cards.rejectReasonPh')}
                  className="mono"
                  style={{ flex: 1, minWidth: 60, fontSize: 11, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px', outline: 'none' }}
                />
                <Btn danger onClick={() => api.rejectProposal(q.id, rejectReasonWithJudge(rejectReason, jv))}>
                  {t('cards.reject')}{top && ` ${rejectTip}`}{mk(jv === 'stamp')}
                </Btn>
              </>
            )
          })()}
        </div>
        {/* 票 19：卡内 diff 手风琴（取代旧「查看 diff」直接跳 tab） */}
        <InlineDiff proposalId={String(p.proposal_id)} />
      </CardShell>
    )
  }

  if (q.kind === 'publish') {
    return (
      <CardShell tone="danger" icon="publish" title={t('cards.publish')}>
        <div className="mono" style={{ fontSize: 12 }}>
          {String(p.baseline ?? 'main')} → <strong>{String(p.remote ?? 'origin')}</strong>
        </div>
        <div style={{ color: 'var(--err)', fontSize: 12, margin: '4px 0 8px' }}>
          {String(p.warning ?? t('cards.publishWarn'))}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          {/* ui-audit 票 02（P0-2）：L3 不可逆不显示批准键提示——
              键盘批准对 publish 无效，提示出现即误导（decisions.ts 守卫同步拦截） */}
          <Btn primary danger onClick={() => api.confirmPublish(q.id)}>{t('cards.publishConfirm')}</Btn>
          <Btn danger onClick={() => api.rejectPublish(q.id)}>{t('cards.reject')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'recovery' && p.sub === 'tool_outcome_unknown') {
    return <ActionRecovery q={q} top={top} />
  }

  if (q.kind === 'recovery' && p.sub === 'tool_action_ready') {
    return (
      <CardShell tone="ask" icon="warn" title={t('cards.recovery')}>
        <div className="dim3">{t('cards.actionReadyHint')}</div>
        <code>{String(p.action_id)}</code>
        <Btn primary onClick={() => api.resumeToolAction(String(p.action_id))}>{t('cards.recover')}{top && ` ${approveTip}`}</Btn>
      </CardShell>
    )
  }

  if (q.kind === 'recovery') {
    return (
      <CardShell tone="ask" icon="warn" title={`${t('cards.recovery')} · ${String(p.stage ?? '')}`}>
        <div className="dim3" style={{ fontSize: 11, margin: '4px 0 8px' }}>
          {t('cards.recoveryHint')}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <Btn primary onClick={() => api.recoverRun(String(p.run_id))}>{t('cards.recover')}{top && ` ${approveTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  // stall-watch 票 02–04：失速卡只有两钮，卡上不改派。再试一次同一段失速
  // 只给一轮——payload.retry=false（或无项目经理）时只渲染「知道了」。
  if (q.kind === 'stall') {
    const branch = String(p.branch ?? '')
    const canRetry = p.retry === true
    return (
      <CardShell tone="flag" icon="warn" title={`${t('cards.stall')} · ${t(`cards.stall_${branch}`, { defaultValue: branch })}`}>
        {p.role != null && <div className="mono dim" style={{ fontSize: 12 }}>{String(p.role)}</div>}
        <div className="dim3" style={{ fontSize: 11, margin: '4px 0 8px' }}>
          {canRetry ? t('cards.stallHint') : t('cards.stallAckOnly')}
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          {canRetry && (
            <Btn primary title={approveTip} onClick={() => api.stallRetry(q.id)}>{t('cards.stallRetry')}{top && ` ${approveTip}`}</Btn>
          )}
          <Btn title={rejectTip} onClick={() => api.stallAck(q.id)}>{t('cards.stallAck')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'grant') {
    return (
      <CardShell tone="flag" icon="shield" title={`${t('cards.grant')} · ${String(p.name ?? '')}`}>
        <div className="dim" style={{ fontSize: 12 }}>
          <span className="mono">{String(p.grant_kind ?? '')}</span>
          <span className="dim3" style={{ marginLeft: 8 }}>{String(p.agent_id ?? '')}</span>
          <div className="dim3" style={{ marginTop: 4 }}>{t('cards.grantHint')}</div>
        </div>
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <Btn primary onClick={() => api.confirmGrant(q.id, true)}>{t('cards.grantRun')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.confirmGrant(q.id, false)}>{t('cards.rejectContinue')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'install') {
    const net = p.net === true
    const creds = p.creds === true
    return (
      <CardShell tone="flag" icon="install" title={`${t('cards.install')} · ${String(p.name ?? '')}`}>
        <div className="dim" style={{ fontSize: 12 }}>
          <div className="mono">{String(p.source ?? '')}</div>
          <div className="mono dim3" style={{ marginTop: 2 }}>
            {String(p.command ?? '')}
            {Array.isArray(p.args) ? ` ${(p.args as unknown[]).join(' ')}` : ''}
          </div>
          <div style={{ marginTop: 4, display: 'flex', gap: 8 }}>
            <span className="chip">{net ? t('cards.installNet') : t('cards.installLocal')}</span>
            {creds && <span className="chip">{t('cards.installCreds')}</span>}
          </div>
          <div className="dim3" style={{ marginTop: 4 }}>{t('cards.installHint')}</div>
        </div>
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <Btn primary onClick={() => api.resolveInstall(q.id, true)}>{t('cards.installRun')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.resolveInstall(q.id, false)}>{t('cards.rejectContinue')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  if (q.kind === 'escalation') {
    const isContext = p.sub === 'context_overflow'
    return (
      <CardShell tone="flag" icon="escalate" title={isContext ? t('cards.contextOverflow') : t('cards.escalation')}>
        {isContext ? (
          <div className="dim" style={{ fontSize: 12 }}>
            <span className="mono">{String(p.role ?? '')}</span> · ~{String(p.est_tokens ?? '?')}/{String(p.cap ?? '?')} tok
            <div style={{ marginTop: 4 }}>{t('cards.contextHint')}</div>
          </div>
        ) : (
          <div className="dim" style={{ fontSize: 12 }}>
            flag <span className="mono">{String(p.flag_id ?? '')}</span> → <span className="mono">{String(p.target ?? '')}</span>
          </div>
        )}
        <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
          <Btn primary onClick={() => api.adjudicateFlag(q.id, true)}>{t('cards.agreeContinue')}{top && ` ${approveTip}`}</Btn>
          <Btn danger onClick={() => api.adjudicateFlag(q.id, false)}>{t('cards.rejectContinue')}{top && ` ${rejectTip}`}</Btn>
        </div>
      </CardShell>
    )
  }

  return (
    <CardShell tone="ask" icon="help" title={q.kind}>
      <pre className="mono dim" style={{ fontSize: 11, margin: 0 }}>{JSON.stringify(p)}</pre>
    </CardShell>
  )
}

function StageArtifacts({ runId }: { runId: string }) {
  const artifacts = useUiStore((s) => s.artifacts)
  const list = artifacts.filter((a) => a.stage_run_id === runId)
  if (!list.length) return null
  return (
    <div style={{ fontSize: 12 }}>
      {list.map((a) => (
        <div key={a.id} className="mono dim" style={{ padding: '1px 0' }}>
          {a.path} <span className="dim3">v{a.version}</span>
        </div>
      ))}
    </div>
  )
}

/** in_review 提案的负责人裁决面（ui-audit-2 票 08 / report B）：复审 agent
 *  从无工具可调 proposals::review——提案卡死 in_review 永不到盖章队列。
 *  票 05 起跟待决卡进同一弹窗；行数据由 PendingDialog 拉进 store，
 *  关掉弹窗也不丢（徽标张数含这些行）。 */
function InReviewCards() {
  const { t } = useTranslation()
  const rows = useUiStore((s) => s.reviewRows)
  const [reasons, setReasons] = useState<Record<string, string>>({})

  return (
    <AnimatePresence initial={false}>
      {rows.map((r) => (
        <MotionCard key={r.id}>
        <CardShell tone="flag" icon="hex" title={`${t('cards.proposalReview')} · ${r.id}`}>
          <div className="dim" style={{ fontSize: 12 }}>
            {t(`cards.surface_${r.surface}`)} · <span className="mono">{r.target}</span>
            <span className="dim3" style={{ marginLeft: 8 }}>{t('cards.byAuthor')} {r.author}</span>
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
            <Btn primary onClick={() => api.reviewProposal(r.id, true, '')}>{t('cards.reviewPass')}</Btn>
            <input
              value={reasons[r.id] ?? ''}
              onChange={(e) => setReasons((m) => ({ ...m, [r.id]: e.target.value }))}
              placeholder={t('cards.rejectReasonPh')}
              className="mono"
              style={{ flex: 1, minWidth: 60, fontSize: 11, background: 'var(--bg-2)', border: '1px solid var(--border)', borderRadius: 6, padding: '3px 8px', outline: 'none' }}
            />
            <Btn danger onClick={() => api.reviewProposal(r.id, false, reasons[r.id] ?? '')}>
              {t('cards.reject')}
            </Btn>
          </div>
          <InlineDiff proposalId={r.id} />
        </CardShell>
        </MotionCard>
      ))}
    </AnimatePresence>
  )
}

export function PendingCards() {
  const { t } = useTranslation()
  const pending = useUiStore((s) => s.pending)
  const sorted = [...pending].sort((a, b) => severityOf(a) - severityOf(b))
  const top = sorted[0]
  const topRef = useRef<HTMLDivElement>(null)
  const [topGone, setTopGone] = useState(false)
  // ui-audit 票 02（P1-3）：顶卡滚出视口时，快捷键仍在作用于它——
  // 顶缘 sticky 迷你条标出目标，让「键会打在哪张卡」始终可见。
  useEffect(() => {
    const el = topRef.current
    if (!el) { setTopGone(false); return }
    const ob = new IntersectionObserver(
      ([e]) => setTopGone(!e.isIntersecting),
      { root: el.closest('#pending-dialog-body'), threshold: 0.4 },
    )
    ob.observe(el)
    return () => ob.disconnect()
  }, [top?.id])
  const approveTip = formatBinding(bindingFor('approve'))
  const rejectTip = formatBinding(bindingFor('reject'))
  return (
    <>
      {topGone && top && (
        <div className="kbd-strip">
          <Icon name="warn" size={11} />
          <span>{t(kindTitleKey(top))}</span>
          <span className="dim3 mono" style={{ marginLeft: 'auto' }}>{approveTip} / {rejectTip}</span>
        </div>
      )}
      <AnimatePresence initial={false}>
        {sorted.map((q, i) => (
          <MotionCard key={q.id} itemRef={i === 0 ? topRef : undefined} current={i === 0}>
            <PendingCard q={q} top={i === 0} />
          </MotionCard>
        ))}
      </AnimatePresence>
      <InReviewCards />
    </>
  )
}

/** 必须人处理的卡的浮层（hands-free 票 05）。不占中栏高度。
 *  关掉只收起：卡仍待决，顶栏徽标留着张数，点徽标再打开。
 *  Esc 不收起——确认层和命令面板已经吃裸 Escape；关闭键进键位表
 *  （默认 mod+Escape），按钮 tooltip 显示当前绑定（ADR 0051）。 */
export function PendingDialog() {
  const { t } = useTranslation()
  const pending = useUiStore((s) => s.pending)
  const reviewRows = useUiStore((s) => s.reviewRows)
  const open = useUiStore((s) => s.pendingDialogOpen)
  const close = useUiStore((s) => s.closePendingDialog)

  useEffect(() => {
    let cancel = false
    api.proposals()
      .then((all) => {
        if (!cancel) useUiStore.getState().noteReviewRows(all.filter((r) => r.status === 'in_review'))
      })
      .catch((e) => {
        if (cancel) return
        useUiStore.getState().noteReviewRows([])
        useUiStore.getState().pushToast(errText(e), 'err')
      })
    return () => { cancel = true }
  }, [pending])

  useEffect(() => {
    useUiStore.getState().syncPendingDialog()
  }, [pending, reviewRows])

  useEffect(() => {
    if (!open) return
    const h = (e: KeyboardEvent) => {
      if (!matches(e, bindingFor('dismissPending'))) return
      const st = useUiStore.getState()
      if (st.modalScope !== 'workbench' || st.confirmReq) return
      e.preventDefault()
      e.stopPropagation()
      st.closePendingDialog()
    }
    window.addEventListener('keydown', h, true)
    return () => window.removeEventListener('keydown', h, true)
  }, [open])

  if (!open || (pending.length === 0 && reviewRows.length === 0)) return null
  const closeTip = `${t('cards.pendingClose')} ${formatBinding(bindingFor('dismissPending'))}`
  return (
    <div
      role="presentation"
      style={{
        position: 'fixed', inset: 0, zIndex: 65,
        background: 'rgba(0,0,0,.45)', display: 'flex',
        alignItems: 'center', justifyContent: 'center',
      }}
      onClick={close}
    >
      <div
        className="panel panel-float"
        role="dialog"
        aria-modal="true"
        aria-label={t('cards.pendingDialog')}
        style={{
          width: 'min(720px, 94vw)', maxHeight: 'min(80vh, 720px)',
          display: 'flex', flexDirection: 'column', padding: '10px 0 12px',
          boxShadow: '0 12px 40px rgba(0,0,0,.4)',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '0 14px 8px' }}>
          <span style={{ fontWeight: 560, fontSize: 13 }}>{t('cards.pendingDialog')}</span>
          <span style={{ flex: 1 }} />
          <button
            className="icon-btn"
            title={closeTip}
            aria-label={t('cards.pendingClose')}
            onClick={close}
          >
            <Icon name="close" size={12} />
          </button>
        </div>
        <div id="pending-dialog-body" style={{ overflowY: 'auto', flex: 1, minHeight: 0 }}>
          <PendingCards />
        </div>
      </div>
    </div>
  )
}
