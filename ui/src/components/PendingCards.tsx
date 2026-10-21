// 决策卡（ui-redesign 票 04，components D8 卡种全家福，decisions#cards）：每个卡种 → 一张 DecisionShell。
// 图标块 = 类别，主按钮颜色 = 后果；顶卡（坞的第一张）按钮内写 ⌘↵ / ⌘⌫ 当前绑定（interaction §2）。
// 动作分发与 decisions.ts 的 approveQuestion / rejectQuestion 一一对应：本票只换皮，卡种与动作不增不减（ADR 0089）。
// 坞本体（分页、收起、modalScope）在 PendingDock.tsx；复杂卡种在 PendingCardParts.tsx。
import { useRef, useState } from 'react'
import { Trans, useTranslation } from 'react-i18next'
import { api, type PendingQuestion } from '../api'
import { useUiStore } from '../store'
import { isActivationResumeCard, policyNeedsQuality, rejectReasonWithJudge } from '../decisions'
import { openReleaseSettings } from '../releaseFocus'
import { ModelRecoveryStatus } from './ModelRecoveryStatus'
import { JudgeModelFailure, RejudgeButton, judgeFailureTitle } from './JudgeModelFailure'
import { PermissionActions } from './PermissionActions'
import { ComputerActionCard } from './ComputerActionCard'
import { DesignChoices } from './DesignChoices'
import { ExperienceProposal } from './ExperienceProposal'
import { StageEvidence } from './StageEvidence'
import { Icon } from './Icon'
import { Checkbox } from './ui/checkbox'
import { DecisionButton, DecisionShell, KeyValues, SettleButton } from './DecisionShell'
import { AcceptanceException, ActionRecovery, InlineDiff, PolicyReport, QualityRevalidation, StageArtifacts } from './PendingCardParts'

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
  return <div className="dock-hint">{lines.map((line) => <div key={line}>{line}</div>)}</div>
}

/** 角色名（花名册里找得到时）——卡头「QA 请求运行命令」的主语。 */
function useWho(agentId: string | null | undefined): string {
  const team = useUiStore((s) => s.team)
  const m = team.find((x) => x.id === agentId)
  return m ? m.role : String(agentId ?? '')
}

/** 命令等宽块：bash 写成 `$ cmd`，其它工具写 key=value。 */
function commandText(p: Record<string, unknown>): string {
  const input = p.input as Record<string, unknown> | undefined
  if (!input) return ''
  const cmd = input.command ?? input.cmd
  if (typeof cmd === 'string') return `$ ${cmd}`
  return Object.entries(input).map(([k, v]) => `${k}=${typeof v === 'string' ? v.slice(0, 120) : JSON.stringify(v)}`).join(' ')
}

function PermissionCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  const who = useWho(q.agent_id)
  const tool = String(p.tool ?? '')
  const safety = Boolean(p.safety_net)
  const judgeTitle = judgeFailureTitle(q, t)
  const failure = p.model_failure as { code?: string } | undefined
  const cmd = commandText(p)
  // 票 16B（control#action）：电脑动作走 D8 专用卡；判定失败卡仍用通用形（标题、重新判定按钮在那里）
  if (tool === 'computer_action' && !judgeTitle) return <ComputerActionCard q={q} top={top} who={who} />
  // owner-followups-1015 #13：Agent 终端 `git push` 画推送卡形（与 ⌘K 推送同一张卡的样子）；判定失败卡仍用通用形。
  const push = p.push as AgentPushFacts | null | undefined
  if (push && !judgeTitle) return <AgentPushCard q={q} top={top} who={who} push={push} />
  return (
    <DecisionShell qid={q.id} cat={judgeTitle ? 'mute' : 'warn'} icon={judgeTitle ? 'warn' : 'shield'}
      title={judgeTitle ?? (who ? t('dock.askTitle', { who, tool }) : t('cards.ask'))}
      sub={judgeTitle
        ? t('dock.judgeSub', { code: String(failure?.code ?? '') })
        : [t('dock.kind.permission'), tool, safety ? t('cards.askSafetyNet') : ''].filter(Boolean).join(' · ')}
      lead={<RejudgeButton q={q} top={top} />}
      note={null}
      actions={<PermissionActions key={q.id} q={q} top={top} />}>
      {judgeTitle && <p>{t('dock.judgeOriginal', { who: who || t('cards.ask'), tool })}</p>}
      {cmd && <code className="dock-cmd">{cmd}</code>}
      {p.reason != null && String(p.reason) !== '' && <p className="dock-hint">{t('dock.reason', { reason: String(p.reason) })}</p>}
      {safety && <div className="dock-row"><span className="chip err">{t('cards.askSafetyNet')}</span></div>}
      {Array.isArray(p.write_targets) && p.write_targets.length > 0 && <p className="dock-hint">
        {t('cards.writeTargets')} <span className="mono">{p.write_targets.filter((v): v is string => typeof v === 'string').join(' · ')}</span>
      </p>}
      <ProvenanceLines payload={p} />
      <JudgeModelFailure q={q} top={top} />
    </DecisionShell>
  )
}

/** 宿主载荷 `push`（`push_class::card_facts`）：普通推送带远程 + 分支，强推带原因码。只用于展示。 */
type AgentPushFacts = { force?: boolean; remote?: string; branch?: string; code?: string }

/** #13：Agent 终端推送。普通推送 = 推送卡形 + 权限底栏（「允许本项目」记 远程 + 分支，宿主给范围时才有 ▾）；
 *  强推 = 安全网：必问、无范围菜单、托管不代批，卡上写明会改写远端历史。 */
function AgentPushCard({ q, top, who, push }: { q: PendingQuestion; top: boolean; who: string; push: AgentPushFacts }) {
  const { t } = useTranslation()
  const force = push.force === true
  const cmd = commandText(q.payload)
  return (
    <DecisionShell qid={q.id} cat="err" icon="publish" danger
      title={who ? t('cards.agentPush', { who }) : t('cards.push')}
      sub={[t('dock.pushSub'), t('cards.pushFromTerminal'), force ? t('cards.askSafetyNet') : ''].filter(Boolean).join(' · ')}
      lead={<RejudgeButton q={q} top={top} />}
      note={null}
      actions={<PermissionActions key={q.id} q={q} top={top} />}>
      {cmd && <code className="dock-cmd">{cmd}</code>}
      {force ? <>
        <div className="dock-row"><span className="chip err">{t('cards.pushForce')}</span></div>
        <div className="dock-warn"><Icon name="warn" size={14} />{t('cards.pushForceWarn')}</div>
      </> : <KeyValues rows={[
        [t('dock.kvTarget'), String(push.remote ?? ''), 'mono'],
        [t('dock.kvBranch'), String(push.branch ?? ''), 'mono'],
      ]} />}
      <ProvenanceLines payload={q.payload} />
      <JudgeModelFailure q={q} top={top} />
    </DecisionShell>
  )
}

function StageStampCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  const rewindStageRef = useRef<HTMLSelectElement>(null)
  const rewindStageInputRef = useRef<HTMLInputElement>(null)
  const revisionNoteRef = useRef<HTMLInputElement>(null)
  const stageNames = useUiStore((s) => s.stages)
  const runId = String(p.run_id ?? '')
  const count = useUiStore((s) => s.artifacts.filter((a) => a.stage_run_id === runId).length)
  const finalGate = p.final_acceptance === true
  const names = [...new Set(stageNames.map((s) => s.stage))]
  return (
    <DecisionShell qid={q.id} cat="warn" icon="stamp" title={t('dock.stampTitle', { stage: String(p.stage ?? '') })}
      sub={[t(finalGate ? 'dock.finalSub' : 'cards.stageStamp'), count ? t('dock.artifactCount', { count }) : ''].filter(Boolean).join(' · ')}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => (finalGate
          ? api.rejectStamp(rewindStageRef.current?.value ?? rewindStageInputRef.current?.value ?? '', revisionNoteRef.current?.value ?? '')
          : api.rejectStamp())}>{t('cards.reject')}</SettleButton>
        <SettleButton qid={q.id} variant="primary" action="approve" showKey={top} run={() => api.stamp()}>{t('dock.stamp')}</SettleButton>
      </>}>
      <StageArtifacts runId={runId} />
      <StageEvidence runId={runId} />
      {finalGate && (
        <div className="dock-row" style={{ flexDirection: 'column', alignItems: 'stretch' }}>
          {names.length > 0 ? (
            <select ref={rewindStageRef} aria-label={t('cards.finalStagePh')} defaultValue="">
              <option value="">{t('cards.finalStagePh')}</option>
              {names.map((n) => <option key={n} value={n}>{n}</option>)}
            </select>
          ) : (
            <input ref={rewindStageInputRef} className="dock-input" aria-label={t('cards.finalStagePh')} placeholder={t('cards.finalStagePh')} />
          )}
          <input ref={revisionNoteRef} className="dock-input" aria-label={t('cards.finalNotePh')} placeholder={t('cards.finalNotePh')} />
        </div>
      )}
    </DecisionShell>
  )
}

function ProposalStampCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  const [rejectReason, setRejectReason] = useState('')
  const policy = p.surface === 'pack_copy'
  const warnings = (p.warnings as string[] | undefined) ?? []
  // 票 07（P2-15）：逆建议留痕——judge=stamp 时驳回、judge=reject 时确认，
  // 按钮挂 againstJudge 小标记；驳回 reason 追加 judge=<verdict> 对照。
  const jv = p.judge_verdict != null ? String(p.judge_verdict) : null
  const against = (bad: boolean) => bad && <span className="dock-against"> {t('cards.againstJudge')}</span>
  const blocked = p.policy_recovery === true || policyNeedsQuality(q)
  const surface = t(`cards.surface_${String(p.surface)}`, { defaultValue: String(p.surface ?? '') })
  const suffix = jv === 'stamp' ? 'Stamp' : jv === 'reject' ? 'Reject' : 'NeedsHuman'
  return (
    <DecisionShell qid={q.id} cat="flag" icon="hex" title={`${t(policy ? 'policy.title' : 'cards.proposalStamp')} · ${String(p.proposal_id)}`}
      sub={`${t('cards.proposalStamp')} · ${String(p.proposal_id)} · ${t('dock.surface', { surface })}`}
      note={blocked ? t('dock.adoptBlocked') : undefined}
      actions={<>
        <input className="dock-input" value={rejectReason} onChange={(e) => setRejectReason(e.target.value)}
          placeholder={t('cards.rejectReasonPh')} aria-label={t('cards.rejectReasonPh')} />
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.rejectProposal(q.id, rejectReasonWithJudge(rejectReason, jv))}>
          {t('cards.reject')}{against(jv === 'stamp')}
        </SettleButton>
        <SettleButton qid={q.id} variant="primary" action="approve" showKey={top} disabled={blocked} run={() => api.confirmProposal(q.id)}>
          {t(policy ? 'policy.adopt' : 'dock.adopt')}{against(jv === 'reject')}
        </SettleButton>
      </>}>
      <div className="dock-hint">{t('cards.proposal')} · {surface}</div>
      {policy && <PolicyReport q={q} />}
      <JudgeModelFailure q={q} top={top} />
      {warnings.length > 0 && (
        <div className="dock-row"><span className="chip warn"><Icon name="warn" size={12} />{warnings.map((w) => t(`cards.warn_${w}`, { defaultValue: w })).join(' · ')}</span></div>
      )}
      {/* 票 09：judge 建议行——闭集 chip + 大白话行（判定是建议不是授权）；票 07（P2-15）视觉降权，不用 ok 绿 / err 红。 */}
      {p.judge_verdict != null && (
        <div className="dock-row">
          <span className="dock-hint" style={{ marginTop: 0 }}>{t('cards.judgeAdvice')}</span>
          <span className={`chip ${jv === 'stamp' ? '' : 'warn'}`}>{t(`cards.judge${suffix}`)}</span>
          <span className="dock-hint" style={{ marginTop: 0 }}>{t(`cards.judgeLine${suffix}`, { rationale: judgeRationale(p, t) })}</span>
          {p.judge_backend != null && <span className="dock-hint" style={{ marginTop: 0 }}>{String(p.judge_backend)}</span>}
        </div>
      )}
      {p.surface === 'skill' && typeof p.proposal_id === 'string' && <ExperienceProposal proposalId={p.proposal_id} />}
      {/* 票 19：卡内 diff 手风琴（取代旧「查看 diff」直接跳 tab） */}
      <InlineDiff proposalId={String(p.proposal_id)} />
    </DecisionShell>
  )
}

function PushCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  // gap A6：「包含 N 个提交 · N 个阶段的产物」取宿主载荷；提交数未知（null，从未推送）只写阶段，旧卡没有计数不出这一行。
  const included = [
    typeof p.commit_count === 'number' ? t('dock.includedCommits', { count: p.commit_count }) : null,
    typeof p.stage_count === 'number' ? t('dock.includedStages', { count: p.stage_count }) : null,
  ].filter((part): part is string => part !== null)
  // owner-followups-1015 #13（ADR 0093 后续裁决）：推送是日常操作，可「允许本项目」——记 远程 + 基线分支，
  // 之后 ⌘K 与 Agent 终端推送同一目标不再问。默认不勾（默认每次问）；键盘批准仍对推送无效。
  const [remember, setRemember] = useState(false)
  const remote = String(p.remote ?? 'origin')
  const baseline = String(p.baseline ?? 'main')
  return (
    <DecisionShell qid={q.id} cat="err" icon="publish" danger title={t('cards.push')} sub={t('dock.pushSub')}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.rejectPush(q.id)}>{t('cards.reject')}</SettleButton>
        {/* ui-audit 票 02（P0-2）：L3 不可逆不显示批准键——键盘批准对 push 无效（decisions.ts 守卫同步拦截） */}
        <SettleButton qid={q.id} variant="danger" run={() => api.confirmPush(q.id, remember)}>{t('cards.pushConfirm')}</SettleButton>
      </>}>
      <KeyValues rows={[
        [t('dock.kvBaseline'), baseline, 'mono'],
        [t('dock.kvTarget'), remote, 'mono'],
        ...(included.length ? [[t('dock.kvIncluded'), included.join(' · ')] as [string, string]] : []),
      ]} />
      <div className="dock-warn"><Icon name="warn" size={14} />{String(p.warning ?? t('cards.pushWarn'))}</div>
      <label className="dock-check"><Checkbox data-remember="push" checked={remember} onCheckedChange={(v) => setRemember(v === true)} />
        {t('cards.pushRemember', { remote, branch: baseline })}</label>
    </DecisionShell>
  )
}

function ReleaseCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const p = q.payload
  const name = String(p.name ?? '')
  const cwd = String(p.cwd ?? '')
  const credentials = Array.isArray(p.credentials) ? p.credentials.map(String) : []
  // owner-followups-1015 #15（components D8「发布确认」）：执行负责人登记的发布目标。必问、永不记忆——卡上没有
  // 「允许本项目」，键盘批准无效（decisions.ts 同步拦截），只认点「确认发布」。卡面抄的是请求时的命令；
  // 登记在此之后被改过，宿主拒绝执行（release_target_changed），负责人批的必须是他看到的那条。
  // 左侧「打开设置·发布」只是导航（补凭据 / 看命令），不是裁决。
  return (
    <DecisionShell qid={q.id} cat="err" icon="publish" danger title={t('cards.release')} sub={t('dock.releaseSub')}
      lead={<DecisionButton variant="ghost" data-release-open onClick={() => openReleaseSettings(String(p.target_id ?? ''))}>
        <Icon name="settings" size={12} />{t('dock.openRelease')}</DecisionButton>}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.rejectRelease(q.id)}>{t('cards.reject')}</SettleButton>
        <SettleButton qid={q.id} variant="danger" data-release-confirm run={async () => {
          await api.confirmRelease(q.id)
          pushToast(t('cards.released', { name }), 'ok')
        }}>{t('cards.releaseConfirm')}</SettleButton>
      </>}>
      <KeyValues rows={[
        [t('dock.kvTarget'), name],
        [t('dock.kvCommand'), String(p.command ?? ''), 'mono'],
        [t('dock.kvCwd'), cwd || t('release.cwdRoot'), cwd ? 'mono' : undefined],
        [t('dock.kvCredentials'), credentials.length ? credentials.join(' · ') : t('dock.credentialsNone'), credentials.length ? 'mono' : undefined],
      ]} />
      <div className="dock-warn"><Icon name="warn" size={14} />{t('cards.releaseWarn')}</div>
    </DecisionShell>
  )
}

function WorktreeCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  // Wayfinder-07 Addendum A: the owner's physical worktree request. Always
  // asked (no "remember"); the card states the absolute path and the bound baseline commit.
  // 票 04（D8 补设计）：红 err 图标块（物理文件系统写入，与发布同档）；清理用 danger、创建用 primary。
  const cleanup = p.operation === 'cleanup'
  const commit = String(p.baseline_commit ?? '').slice(0, 8)
  return (
    <DecisionShell qid={q.id} cat="err" icon="branch" title={t(cleanup ? 'cards.worktreeCleanup' : 'cards.worktreeCreate')}
      sub={`${t('dock.worktreeSub')} · ${t(cleanup ? 'dock.opCleanup' : 'dock.opCreate')}`}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.rejectWorktree(q.id)}>{t('cards.reject')}</SettleButton>
        <SettleButton qid={q.id} variant={cleanup ? 'danger' : 'primary'} action="approve" showKey={top} run={() => api.confirmWorktree(q.id)}>
          {t(cleanup ? 'cards.worktreeCleanupConfirm' : 'cards.worktreeCreateConfirm')}
        </SettleButton>
      </>}>
      <KeyValues rows={[
        [t('dock.kvOperation'), t(cleanup ? 'dock.opCleanup' : 'dock.opCreate')],
        [t('dock.kvPath'), <span key="path" title={String(p.path ?? '')}>{String(p.path ?? '')}</span>, 'mono'],
        [t('dock.kvBaseline'), t('cards.worktreeBaseline', { baseline: String(p.baseline ?? ''), commit }), 'mono'],
        ...(p.session_id != null ? [[t('dock.kvSession'), String(p.session_id), 'mono'] as [string, string, string]] : []),
      ]} />
      {cleanup && <p className="dock-hint">{t('cards.worktreeCleanupNote')}</p>}
    </DecisionShell>
  )
}

function StallCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  // stall-watch 票 02–04：失速卡只有两钮，卡上不改派。再试一次同一段失速
  // 只给一轮——payload.retry=false（或无项目经理）时只渲染「知道了」。
  const branch = String(p.branch ?? '')
  const canRetry = p.retry === true
  // I2 / 2026-10-06: admission did not run a retrigger. The refusal already
  // took effect; this card offers only continuation of its original task.
  // 票 04（D8「失速 · 模型不可用」）：左侧「打开模型服务」只是导航，不是裁决。
  const resumeWaiting = isActivationResumeCard(q)
  return (
    <DecisionShell qid={q.id} cat="flag" icon={resumeWaiting ? 'pause' : 'clock'}
      title={resumeWaiting ? t('cards.resumeWaiting') : `${t('cards.stall')} · ${t(`cards.stall_${branch}`, { defaultValue: branch })}`}
      sub={[resumeWaiting ? t('dock.modelUnavailable') : t('cards.stall'), p.role != null ? String(p.role) : ''].filter(Boolean).join(' · ')}
      lead={resumeWaiting && <DecisionButton variant="ghost" onClick={() => window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: 'models' }))}>
        <Icon name="cpu" size={12} />{t('dock.openModels')}</DecisionButton>}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.stallAck(q.id)}>{t('cards.stallAck')}</SettleButton>
        {canRetry && <SettleButton qid={q.id} variant="primary" action="approve" showKey={top} run={() => api.stallRetry(q.id)}>
          {resumeWaiting ? t('dock.continueOriginal') : t('cards.stallRetry')}
        </SettleButton>}
      </>}>
      <p>{resumeWaiting ? t('cards.resumeWaitingHint') : canRetry ? t('cards.stallHint') : t('cards.stallAckOnly')}</p>
    </DecisionShell>
  )
}

function InstallCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  const cmd = `${String(p.command ?? '')}${Array.isArray(p.args) ? ` ${(p.args as unknown[]).join(' ')}` : ''}`
  return (
    <DecisionShell qid={q.id} cat="warn" icon="install" title={t('dock.installTitle', { name: String(p.name ?? '') })}
      sub={[t('dock.installSub'), String(p.source ?? '')].filter(Boolean).join(' · ')}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.resolveInstall(q.id, false)}>{t('cards.rejectContinue')}</SettleButton>
        <SettleButton qid={q.id} variant="primary" action="approve" showKey={top} run={() => api.resolveInstall(q.id, true)}>{t('cards.installRun')}</SettleButton>
      </>}>
      {cmd.trim() && <code className="dock-cmd">{cmd}</code>}
      <div className="dock-row">
        <span className={`chip ${p.net === true ? 'warn' : ''}`}><Icon name="web" size={12} />{p.net === true ? t('cards.installNet') : t('cards.installLocal')}</span>
        {p.creds === true && <span className="chip"><Icon name="key" size={12} />{t('cards.installCreds')}</span>}
        <span className="dock-hint" style={{ marginTop: 0 }}>{t('cards.installHint')}</span>
      </div>
    </DecisionShell>
  )
}

function GrantCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  return (
    <DecisionShell qid={q.id} cat="warn" icon="shield" title={`${t('cards.grant')} · ${String(p.name ?? '')}`}
      sub={[String(p.grant_kind ?? ''), String(p.agent_id ?? '')].filter(Boolean).join(' · ')}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.confirmGrant(q.id, false)}>{t('cards.rejectContinue')}</SettleButton>
        <SettleButton qid={q.id} variant="primary" action="approve" showKey={top} run={() => api.confirmGrant(q.id, true)}>{t('cards.grantRun')}</SettleButton>
      </>}>
      <p>{t('cards.grantHint')}</p>
    </DecisionShell>
  )
}

function EscalationCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  const isContext = p.sub === 'context_overflow'
  const est = Number(p.est_tokens)
  const cap = Number(p.cap)
  const pct = Number.isFinite(est) && Number.isFinite(cap) && cap > 0 ? Math.min(100, Math.round((est / cap) * 100)) : null
  return (
    <DecisionShell qid={q.id} cat="flag" icon="escalate" title={isContext ? t('cards.contextOverflow') : t('cards.escalation')}
      sub={[t('dock.escalationSub'), isContext ? String(p.role ?? '') : ''].filter(Boolean).join(' · ')}
      actions={<>
        <SettleButton qid={q.id} variant="quiet" action="reject" showKey={top} run={() => api.adjudicateFlag(q.id, false)}>{t('cards.rejectContinue')}</SettleButton>
        <SettleButton qid={q.id} variant="primary" action="approve" showKey={top} run={() => api.adjudicateFlag(q.id, true)}>{t('cards.agreeContinue')}</SettleButton>
      </>}>
      {isContext ? (
        <>
          <div className="dock-meter">
            <span className="dock-hint" style={{ marginTop: 0, width: 64 }}>{t('dock.estimate')}</span>
            <i>{pct != null && <b style={{ width: `${pct}%` }} />}</i>
            <span className="mono">{t('cards.contextTokens', { est: String(p.est_tokens ?? '?'), cap: String(p.cap ?? '?') })}</span>
          </div>
          <p className="dock-hint">{t('cards.contextHint')}</p>
        </>
      ) : (
        <p><Trans i18nKey="cards.flagRoute" values={{ flag: String(p.flag_id ?? ''), target: String(p.target ?? '') }}
          components={{ m: <span className="mono" /> }} /></p>
      )}
    </DecisionShell>
  )
}

function RecoveryCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  const ready = p.sub === 'tool_action_ready'
  return (
    <DecisionShell qid={q.id} cat="warn" icon="undo"
      title={ready ? t('cards.recovery') : t('dock.recoveryTitle', { stage: String(p.stage ?? '') })}
      sub={t('dock.recoverySub')}
      actions={<SettleButton qid={q.id} variant="primary" action="approve" showKey={top}
        run={() => (ready ? api.resumeToolAction(String(p.action_id)) : api.recoverRun(String(p.run_id)))}>{t('cards.recover')}</SettleButton>}>
      {ready ? <><p>{t('cards.actionReadyHint')}</p><code className="dock-cmd">{String(p.action_id)}</code></> : <p>{t('cards.recoveryHint')}</p>}
    </DecisionShell>
  )
}

export function PendingCard({ q, top }: { q: PendingQuestion; top: boolean }) {
  const { t } = useTranslation()
  const p = q.payload
  if (q.kind === 'recovery' && p.sub === 'model_request') {
    return <DecisionShell qid={q.id} cat="warn" icon="warn" title={t(p.purpose === 'turn' ? 'modelRecovery.titleTurn' : 'modelRecovery.title')} sub={t('dock.modelRequestSub')}>
      <ModelRecoveryStatus id={q.id} payload={p} top={top} />
    </DecisionShell>
  }
  if (q.kind === 'permission') return <PermissionCard q={q} top={top} />
  if (q.kind === 'stamp' && p.sub === 'acceptance_exception') return <AcceptanceException q={q} top={top} />
  if (q.kind === 'stamp' && p.sub === 'quality_revalidation') return <QualityRevalidation q={q} top={top} />
  if (q.kind === 'stamp' && !p.proposal_id) return <StageStampCard q={q} top={top} />
  if (q.kind === 'stamp') return <ProposalStampCard q={q} top={top} />
  if (q.kind === 'push') return <PushCard q={q} top={top} />
  if (q.kind === 'release') return <ReleaseCard q={q} top={top} />
  if (q.kind === 'worktree') return <WorktreeCard q={q} top={top} />
  if (q.kind === 'recovery' && p.sub === 'tool_outcome_unknown') return <ActionRecovery q={q} top={top} />
  if (q.kind === 'recovery') return <RecoveryCard q={q} top={top} />
  if (q.kind === 'stall') return <StallCard q={q} top={top} />
  if (q.kind === 'grant') return <GrantCard q={q} top={top} />
  if (q.kind === 'install') return <InstallCard q={q} top={top} />
  if (q.kind === 'escalation' && p.sub === 'design_direction') return <DesignChoices key={q.id} q={q} />
  if (q.kind === 'escalation') return <EscalationCard q={q} top={top} />
  return (
    <DecisionShell qid={q.id} cat="mute" icon="help" title={q.kind}>
      <code className="dock-cmd">{JSON.stringify(p)}</code>
    </DecisionShell>
  )
}
