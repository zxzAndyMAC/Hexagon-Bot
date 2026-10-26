// 决策卡的复杂卡种与卡内部件（ui-redesign 票 04 从 PendingCards.tsx 拆出，components D8）。
// 未知动作、验收例外、质量重新验收、待复审提案，以及卡内 diff 手风琴、产物行、策略报告。
// 动作与守卫原样搬迁（ADR 0077 / 0069 / Owner Q4）：本票只换卡壳与按钮外观（D6 / D8），不增删裁决能力。
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import i18n from '../i18n'
import { api, errText, type PendingQuestion } from '../api'
import type { ProposalRow } from '../gen/ProposalRow'
import type { ReconcileReport } from '../gen/ReconcileReport'
import { useUiStore } from '../store'
import { extractDiffBlock, parseUnifiedDiff, type DiffOp } from '../diff'
import { bindingFor, formatBinding, matches } from '../keymap'
import { policyQualityMessage } from '../decisions'
import { exceptionLabel } from '../evidenceLabels'
import { useStageEvidence } from '../useStageEvidence'
import { DiffView } from './DiffView'
import { ExperienceProposal } from './ExperienceProposal'
import { StageEvidence } from './StageEvidence'
import { Icon } from './Icon'
import { FileContextMenu, useProjectFileTarget } from './FileMenu'
import type { ArtifactRow } from '../gen/ArtifactRow'
import { Checkbox } from './ui/checkbox'
import { Field, Input, Textarea } from './ui/input'
import { DecisionButton, DecisionShell, KeyValues, SettleButton } from './DecisionShell'

// 票 19（方向卡 4）：提案卡内嵌 diff 手风琴——chevron 展开懒跑
// proposals→artifactContent→extractDiffBlock 链；限高 200px 内滚 +
// 「打开完整对照」tab 链接。加载失败回退开 tab 路径 + toast（票 04 约定）。
export function InlineDiff({ proposalId }: { proposalId: string }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const pushToast = useUiStore((s) => s.pushToast)
  const [open, setOpen] = useState(false)
  const [loading, setLoading] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [ops, setOps] = useState<DiffOp[] | null>(null)

  // Benchmark I1 / 2026-10-06 review: late A reads previously opened tabs in B.
  // Guard both awaits, so A's artifact path cannot be looked up in B either.
  const fetchDiff = async (epoch: number): Promise<{ diff: string | null; artPath: string | null }> => {
    const props = await api.proposals()
    if (useUiStore.getState().projectEpoch !== epoch) return { diff: null, artPath: null }
    const pr = props.find((x) => String(x.id) === proposalId)
    const ap = pr?.artifact_path ? String(pr.artifact_path) : null
    if (!ap) return { diff: null, artPath: null }
    const body = await api.artifactContent(ap)
    if (useUiStore.getState().projectEpoch !== epoch) return { diff: null, artPath: null }
    return { diff: extractDiffBlock(body), artPath: ap }
  }

  const openFull = async () => {
    const epoch = useUiStore.getState().projectEpoch
    try {
      const { diff, artPath: ap } = await fetchDiff(epoch)
      if (useUiStore.getState().projectEpoch !== epoch) return
      if (diff) {
        openTab({ id: `patch:${proposalId}`, kind: 'diff', title: `${proposalId} diff`, patchText: diff })
      } else if (ap) {
        openTab({ id: `art:${ap}`, kind: 'artifact', title: ap, path: ap })
      }
    } catch (e) {
      if (useUiStore.getState().projectEpoch === epoch) pushToast(errText(e), 'err')
    }
  }

  const toggle = async () => {
    if (open) { setOpen(false); return }
    setOpen(true)
    if (loaded || loading) return
    const epoch = useUiStore.getState().projectEpoch
    setLoading(true)
    try {
      const { diff } = await fetchDiff(epoch)
      if (useUiStore.getState().projectEpoch !== epoch) return
      setOps(diff ? parseUnifiedDiff(diff) : null)
      setLoaded(true)
    } catch (e) {
      if (useUiStore.getState().projectEpoch !== epoch) return
      setOpen(false)
      pushToast(errText(e), 'err')
      void openFull() // 回退开 tab 路径（其内部再 toast）
    } finally {
      if (useUiStore.getState().projectEpoch === epoch) setLoading(false)
    }
  }

  return (
    <div className="dock-diff">
      <div className="dock-row">
        <DecisionButton variant="ghost" title={t('cards.inlineDiff')} aria-expanded={open} onClick={() => void toggle()}>
          <Icon name="diff" size={12} />{t('cards.inlineDiff')}
        </DecisionButton>
        {loaded && <DecisionButton variant="ghost" onClick={() => void openFull()}>{t('cards.openFullDiff')}</DecisionButton>}
      </div>
      {open && (
        <div className="dock-diff-body">
          {loading
            ? <div className="dock-hint">…</div>
            : ops
              ? <DiffView ops={ops} />
              : <div className="dock-hint">{t('cards.noDiff')}</div>}
        </div>
      )}
    </div>
  )
}

/** 盖章卡的证据前置（decisions#cards）：产物行 = circle-check 绿 + 路径等宽 + vN。 */
export function StageArtifacts({ runId }: { runId: string }) {
  const artifacts = useUiStore((s) => s.artifacts)
  const list = artifacts.filter((a) => a.stage_run_id === runId)
  if (!list.length) return null
  return (
    <div className="dock-list">
      {list.map((a) => <StageArtifactRow key={a.id} a={a} />)}
    </div>
  )
}

/**
 * 阶段产物证据行；右键 = 共用文件菜单的路径类项（#32，2026-10-21 负责人裁决）——只有文件项：
 * 没有「打开」（此表面本来没有打开动作，打开走画布产物页签），更没有任何裁决项（A16b 禁区：误点代价是一次未审副作用）。
 * 待恢复（盘上未写成）的行一项不剩 → 不挂菜单。
 */
function StageArtifactRow({ a }: { a: ArtifactRow }) {
  const target = useProjectFileTarget(a.materialization === 'pending_recovery' ? null : a.path)
  return (
    <FileContextMenu {...target}>
      <div data-stage-artifact>
        <Icon name="circle-check" size={14} style={{ color: 'var(--green)' }} />
        <span className="mono" title={a.path}>{a.path}</span>
        <span className="chip">v{a.version}</span>
      </div>
    </FileContextMenu>
  )
}

export function PolicyReport({ q }: { q: PendingQuestion }) {
  const { t } = useTranslation()
  const openButton = useRef<HTMLButtonElement>(null)
  const report = q.payload.evidence as Record<string, unknown> | null | undefined
  return <div onKeyDown={(e) => {
    if (!matches(e, bindingFor('openPolicyReport'))) return
    e.preventDefault()
    if (!e.repeat && useUiStore.getState().modalScope === 'workbench') openButton.current?.click()
  }}>
    <p>{t('policy.ownerOnly')}</p>
    <p role="status">{t(policyQualityMessage(q))}</p>
    {q.payload.policy_recovery === true && <p role="alert">{t('policy.recovery')}</p>}
    {report && <p className="mono">{t('policy.scores', { baseline: report.baseline_score ?? '—', candidate: report.candidate_score ?? '—' })}</p>}
    <div className="dock-row">
      <DecisionButton ref={openButton} variant="ghost" title={`${t('policy.report')} · ${formatBinding(bindingFor('openPolicyReport'))}`} onClick={async () => {
        const epoch = useUiStore.getState().projectEpoch
        try {
          const proposal = (await api.proposals()).find((p) => p.id === q.payload.proposal_id)
          if (useUiStore.getState().projectEpoch !== epoch) return
          if (!proposal?.artifact_path) throw new Error(t('errors.not_found'))
          useUiStore.getState().openTab({ id: `art:${proposal.artifact_path}`, kind: 'artifact', title: proposal.artifact_path, path: proposal.artifact_path })
          useUiStore.getState().closePendingDialog()
        } catch (e) { if (useUiStore.getState().projectEpoch === epoch) useUiStore.getState().pushToast(errText(e), 'err') }
      }}>{t('policy.report')}</DecisionButton>
    </div>
  </div>
}

type RecoveryAction = 'reconcileAction' | 'abandonAction' | 'retryAction'

// Test3/13 (spec 06): the unknown-action card states what was attempted,
// whether a read-only check exists, why a button is unavailable, and the
// result of every check — even when the evidence did not change.
// 票 04（R-03 拍板，components D8「未知动作」）：动作照 ADR 0077 的三个——核对（default，支持核对时出现）·
// 放弃追查（quiet，须勾「我了解放弃追查后不会重放该动作」）· 重试（default，仍须接受重复副作用风险，
// core 的 retry 判定要求 accepts_duplicate）；理由必填，空时后两者禁用。原型的「标记已解决」没有后端语义，不加。
export function ActionRecovery({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  const team = useUiStore((s) => s.team)
  const [reason, setReason] = useState('')
  const [acceptsDuplicate, setAcceptsDuplicate] = useState(false)
  const [understandsAbandon, setUnderstandsAbandon] = useState(false)
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)
  const [report, setReport] = useState<ReconcileReport | null>((p.last_reconcile as ReconcileReport | undefined) ?? null)
  const container = useRef<HTMLDivElement>(null)
  const id = String(p.action_id)
  const projectRoot = typeof p.host_project_root === 'string' ? p.host_project_root : ''
  const supported = p.reconcile_supported !== false
  const run = async (action: () => Promise<unknown>) => {
    if (inFlight.current) return
    if (useUiStore.getState().modalScope !== 'workbench') { useUiStore.getState().pushToast(i18n.t('decisions.scopeBlocked')); return }
    inFlight.current = true; setBusy(true)
    try { await action() } catch (e) {
      useUiStore.getState().pushToast(i18n.t('errors.actionFailed', { detail: errText(e) }), 'err')
    } finally {
      try { await useUiStore.getState().invalidate() } finally { inFlight.current = false; setBusy(false) }
    }
  }
  const reconcile = () => run(async () => {
    const next = await api.reconcileToolAction(id)
    setReport(next)
    useUiStore.getState().pushToast(t('actionRecovery.checked', { n: next.check_count, verdict: t(`actionRecovery.verdict.${next.verdict}`) }))
  })
  const abandon = () => run(async () => {
    await api.abandonToolAction(id, reason, projectRoot)
    await api.offerContinueAfterUnknown(id).catch(() => false)
  })
  const retry = () => run(() => api.retryToolAction(id, reason, acceptsDuplicate))
  const needReason = !reason.trim()
  const abandonBlock = needReason ? 'needReason' : !projectRoot ? 'projectUnknown' : !understandsAbandon ? 'needAbandonAck' : ''
  const retryBlock = needReason ? 'needReason' : !acceptsDuplicate ? 'needRisk' : ''
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!top || e.repeat || e.defaultPrevented) return
      const action = (['reconcileAction', 'abandonAction', 'retryAction'] as const).find((a) => matches(e, bindingFor(a)))
      if (!action) return
      e.preventDefault()
      const button = container.current?.querySelector<HTMLButtonElement>(`button[data-action="${action}"]`)
      if (!button || button.disabled) {
        useUiStore.getState().pushToast(t(button ? `actionRecovery.blocked.${button.dataset.block}` : 'actionRecovery.noCheck'))
        return
      }
      button.click()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [top, t])
  const label = (action: RecoveryAction, text: string, block: string) =>
    [text, block ? t(`actionRecovery.blocked.${block}`) : '', formatBinding(bindingFor(action))].filter(Boolean).join(' · ')
  const who = team.find((m) => m.id === q.agent_id)
  const firstBlock = abandonBlock || retryBlock
  const action = (name: RecoveryAction, variant: 'default' | 'quiet', text: string, block: string, onClick: () => void) => (
    <DecisionButton variant={variant} data-action={name} data-block={block} disabled={busy || !!block}
      title={label(name, text, block)} showKey={top} action={name} onClick={onClick}>{text}</DecisionButton>
  )
  return <div ref={container} aria-busy={busy} style={{ display: 'contents' }}>
    <DecisionShell qid={q.id} cat="mute" icon="warn" title={t('cards.actionUnknown')}
      sub={[t('dock.unknownSub'), t('actionRecovery.errorClass', { code: String(p.error_code ?? t('actionRecovery.errorUnknown')) }), t(supported ? 'dock.canCheck' : 'dock.noCheck')].join(' · ')}
      lead={supported && action('reconcileAction', 'default', busy ? t('actionRecovery.working') : t('cards.reconcileAction'), '', () => { void reconcile() })}
      note={firstBlock ? <span data-blocked="">{t(`actionRecovery.blocked.${firstBlock}`)}</span> : undefined}
      actions={<>
        {action('abandonAction', 'quiet', t('cards.abandonAction'), abandonBlock, () => { void abandon() })}
        {action('retryAction', 'default', t('cards.retryAction'), retryBlock, () => { void retry() })}
      </>}>
      <KeyValues wide rows={[
        [t('dock.kvWho'), who ? `${who.role} [${who.id}]` : String(q.agent_id ?? '—')],
        [t('dock.kvWhat'), <span data-action-facts=""><span className="mono">{String(p.tool ?? t('actionRecovery.toolUnknown'))}</span>{p.target ? <span className="mono"> · {String(p.target)}</span> : null}</span>],
        [t('dock.kvCanCheck'), <span data-capability="">{t(supported ? 'actionRecovery.canCheck' : 'actionRecovery.noCheck')}</span>],
      ]} />
      <p className="dock-hint">{t('cards.actionUnknownHint')}</p>
      {report && <p role="status" data-last-check="" className="dock-hint">
        {t('actionRecovery.lastCheck', { n: report.check_count, verdict: t(`actionRecovery.verdict.${report.verdict}`), reason: t(`actionRecovery.reason.${report.reason}`, { defaultValue: report.reason }) })}
        {report.continue_offered && ` ${t('actionRecovery.continueOffered')}`}
      </p>}
      <details className="dock-hint">
        <summary>{t('actionRecovery.technical')}</summary>
        <code>{id}</code>
        {p.reconciliation_evidence != null && <p>{String(p.reconciliation_evidence)}</p>}
      </details>
      <Field className="dock-field" label={t('dock.reasonRequired')} help={t('actionRecovery.reasonHint')}>
        <Input value={reason} aria-invalid={needReason || undefined} placeholder={t('dock.reasonPh')} onChange={(e) => setReason(e.target.value)} />
      </Field>
      <label className="dock-check"><Checkbox data-ack="abandon" checked={understandsAbandon} onCheckedChange={(v) => setUnderstandsAbandon(v === true)} />{t('dock.abandonAck')}</label>
      <label className="dock-check"><Checkbox data-ack="duplicate" checked={acceptsDuplicate} onCheckedChange={(v) => setAcceptsDuplicate(v === true)} />{t('cards.duplicateRisk')}</label>
    </DecisionShell>
  </div>
}

export function AcceptanceException({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const { evidence, error } = useStageEvidence(q.id)
  const [reason, setReason] = useState('')
  const [selected, setSelected] = useState<string[]>([])
  const container = useRef<HTMLDivElement>(null)
  useEffect(() => { setSelected([]) }, [evidence])
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (!top || e.repeat || e.defaultPrevented || !matches(e, bindingFor('acceptException'))) return
      e.preventDefault()
      if (useUiStore.getState().modalScope !== 'workbench') {
        useUiStore.getState().pushToast(i18n.t('decisions.scopeBlocked'))
        return
      }
      container.current?.querySelector<HTMLButtonElement>('button[data-variant="primary"]')?.click()
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [top])
  const current = evidence?.run_id === q.payload.run_id && evidence?.fingerprint === q.payload.fingerprint
  const candidates = evidence?.exceptions.filter((item) => !item.accepted) ?? []
  const items = candidates.filter((item) => selected.includes(JSON.stringify(item.requirement)))
  return <div ref={container} style={{ display: 'contents' }}>
    <DecisionShell qid={q.id} cat="warn" icon="warn" title={t('exceptions.title')} sub={String(q.payload.stage ?? '')}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.cancelAcceptanceException(q.id)}>{t('exceptions.cancel')}</SettleButton>
        <SettleButton qid={q.id} variant="primary" action="acceptException" showKey={top}
          disabled={!current || !reason.trim() || items.length === 0 || evidence?.missing.some((m) => m.startsWith('action:'))}
          run={() => api.acceptDeliveryException(q.id, String(q.payload.fingerprint), items.map((item) => item.requirement), reason)}>{t('exceptions.accept')}</SettleButton>
      </>}>
      <p>{t('exceptions.hint')}</p>
      {error && <div role="alert" className="dock-hint" style={{ color: 'var(--red-ink)' }}>{error}</div>}
      {evidence && !current && <p role="alert">{t('exceptions.stale')}</p>}
      {current && candidates.map((item) => {
        const id = JSON.stringify(item.requirement)
        return <label key={id} className="dock-check">
          <Checkbox checked={selected.includes(id)} onCheckedChange={(v) => setSelected((old) => v === true ? [...old, id] : old.filter((value) => value !== id))} />{exceptionLabel(item)}
        </label>
      })}
      <Field className="dock-field" label={t('exceptions.reason')}>
        <Textarea value={reason} onChange={(e) => setReason(e.target.value)} />
      </Field>
    </DecisionShell>
  </div>
}

export function QualityRevalidation({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const { evidence, error } = useStageEvidence(q.id)
  const container = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (!top || e.repeat || e.defaultPrevented) return
      const action = matches(e, bindingFor('confirmQualityRevalidation')) ? 'confirm' : matches(e, bindingFor('cancelQualityRevalidation')) ? 'cancel' : null
      if (!action) return
      e.preventDefault()
      if (useUiStore.getState().modalScope !== 'workbench') { useUiStore.getState().pushToast(i18n.t('decisions.scopeBlocked')); return }
      container.current?.querySelector<HTMLButtonElement>(`button[data-variant="${action === 'confirm' ? 'primary' : 'quiet'}"]`)?.click()
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [top])
  const current = evidence?.run_id === q.payload.run_id && !!evidence?.fingerprint
  const root = String(q.payload.project_root ?? '')
  return <div ref={container} style={{ display: 'contents' }}>
    <DecisionShell qid={q.id} cat="info" icon="test" title={t('quality.revalidationTitle')} sub={t('dock.qualitySub')}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="cancelQualityRevalidation" showKey={top}
          title={`${t('quality.revalidationCancel')} · ${formatBinding(bindingFor('cancelQualityRevalidation'))}`}
          run={() => api.cancelQualityRevalidation(q.id, root)}>{t('quality.revalidationCancel')}</SettleButton>
        <SettleButton qid={q.id} variant="primary" action="confirmQualityRevalidation" showKey={top} disabled={!current || !!evidence?.missing.length}
          title={`${t('quality.revalidationConfirm')} · ${formatBinding(bindingFor('confirmQualityRevalidation'))}`}
          run={() => api.confirmQualityRevalidation(q.id, evidence?.fingerprint ?? '', root)}>{t('quality.revalidationConfirm')}</SettleButton>
      </>}>
      <p>{t('quality.revalidationHint')}</p>
      <StageArtifacts runId={String(q.payload.run_id ?? '')} />
      <StageEvidence runId={String(q.payload.run_id ?? '')} />
      {error && <div role="alert" className="dock-hint" style={{ color: 'var(--red-ink)' }}>{error}</div>}
    </DecisionShell>
  </div>
}

/** in_review 提案的负责人裁决面（ui-audit-2 票 08 / report B）：复审 agent
 *  从无工具可调 proposals::review——提案卡死 in_review 永不到盖章队列。
 *  票 04 起是待决坞的最后几页（排在全部待决卡之后，不吃 ⌘↵ / ⌘⌫）。 */
export function ReviewCard({ row }: { row: ProposalRow }) {
  const { t } = useTranslation()
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState(false)
  const decide = (pass: boolean) => {
    if (busy) return
    setBusy(true)
    void api.reviewProposal(row.id, pass, pass ? '' : reason)
      .then(() => useUiStore.getState().invalidate())
      .catch((e) => useUiStore.getState().pushToast(i18n.t('errors.actionFailed', { detail: errText(e) }), 'err'))
      .finally(() => setBusy(false))
  }
  return (
    <DecisionShell cat="flag" icon="hex" title={`${t('cards.proposalReview')} · ${row.id}`}
      sub={`${t(`cards.surface_${row.surface}`)} · ${row.target} · ${t('cards.byAuthor')} ${row.author}`}
      actions={<>
        <input className="dock-input" value={reason} onChange={(e) => setReason(e.target.value)} placeholder={t('cards.rejectReasonPh')} aria-label={t('cards.rejectReasonPh')} />
        <DecisionButton variant="quiet" busy={busy} onClick={() => decide(false)}>{t('cards.reject')}</DecisionButton>
        <DecisionButton variant="primary" busy={busy} onClick={() => decide(true)}>{t('cards.reviewPass')}</DecisionButton>
      </>}>
      {row.surface === 'skill' && <ExperienceProposal proposalId={row.id} />}
      <InlineDiff proposalId={row.id} />
    </DecisionShell>
  )
}
