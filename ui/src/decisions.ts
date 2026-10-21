// 待决卡的领域逻辑：严重度序 + 各 kind 的批准/驳回分发（ADR 0051）。
import { api, errText, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { bindingFor, matches } from './keymap'
import i18n from './i18n'
import { toastError } from './toast'

// 严重度序：恢复/失速/发布/安装 > 阶段盖章 > 升级 > 权限 > 提案盖章
// 失速与恢复同档：两者都是「工作停了，不点不会自己动」（ADR 0074）。
const SEVERITY: Record<string, number> = {
  recovery: 0,
  stall: 0,
  push: 0,
  release: 0,
  worktree: 0,
  install: 0,
  grant: 0,
  stamp_stage: 1,
  escalation: 2,
  permission: 3,
  stamp_proposal: 4,
}

export function severityOf(q: PendingQuestion): number {
  if (q.kind === 'stamp') return q.payload.proposal_id ? SEVERITY.stamp_proposal : SEVERITY.stamp_stage
  return SEVERITY[q.kind] ?? 9
}

/** 坞里看得见的卡，按严重度稳定排序（同档保持入队序）。顶卡 = [0] = ⌘↵ / ⌘⌫ 的作用对象；
 *  「保存即消卡」途中的卡（hiddenPending）不算——键只打在负责人眼前那张上（票 04，ADR 0089）。 */
export function visiblePending(pending: readonly PendingQuestion[], hidden: readonly string[] = []): PendingQuestion[] {
  return pending
    .map((q, order) => ({ q, order }))
    .filter(({ q }) => !hidden.includes(q.id))
    .sort((a, b) => severityOf(a.q) - severityOf(b.q) || a.order - b.order)
    .map(({ q }) => q)
}

/** 票 04「保存即消卡」（interaction §2、components D6 加载 / 错误态）：裁决一提交，卡先离坞；
 *  读回（invalidate）后解除隐藏——已解决的卡随 pending 消失，没解决的（如最终验收证据不全）原样回来。
 *  失败：卡回到坞顶，底栏上方红字原因（pendingError），不再只靠 toast。返回是否成功。 */
export async function settleDecision(id: string, run: () => Promise<unknown>): Promise<boolean> {
  const st = useUiStore.getState()
  st.hidePending(id)
  let failure: string | null = null
  let caught: unknown
  try {
    await run()
    if (useUiStore.getState().pendingError?.id === id) useUiStore.getState().notePendingError(null)
  } catch (e) {
    caught = e
    failure = i18n.t('errors.actionFailed', { detail: errText(e) })
    useUiStore.getState().notePendingError({ id, text: failure })
  } finally {
    try { await useUiStore.getState().invalidate() } catch { /* 读回失败由状态轮询的 statusError 呈现 */ }
    useUiStore.getState().unhidePending(id)
  }
  // 失败但卡已不在（例：工作树确认时事实漂移，core 收起了卡——D8 `worktree_binding_drift`）：
  // 红字无处可挂，原因走 toast，不能静默。
  // 票 13：toast 写已登记原因码（toastError），漂移时即「…（worktree_binding_drift）」。
  if (failure && !useUiStore.getState().pending.some((q) => q.id === id)) toastError(caught, { wrap: 'errors.actionFailed' })
  return failure == null
}

// Evaluation 16/D14: missing legacy metadata grants no quality authority.
export function policyNeedsQuality(q: PendingQuestion): boolean {
  return (q.payload.surface === 'pack_copy' || q.payload.policy_candidate === true) && q.payload.policy_quality !== 'qualified'
}

export function policyQualityMessage(q: PendingQuestion) {
  switch (q.payload.policy_quality) {
    case 'incomplete': return 'policy.incomplete'
    case 'failed': return 'policy.failed'
    case 'stale': return 'policy.stale'
    case 'qualified': return 'policy.qualified'
    default: return 'policy.unverified'
  }
}

export async function approveQuestion(q: PendingQuestion) {
  const p = q.payload
  // Owner Q4 (2026-10-05): revalidation needs the displayed evidence version;
  // the generic stage-stamp shortcut previously supplied neither card nor version.
  if (p.sub === 'quality_revalidation') { useUiStore.getState().pushToast(i18n.t('quality.revalidationHint')); return }
  if (p.sub === 'design_direction') { useUiStore.getState().pushToast(i18n.t('design.chooseHint')); return }
  if (p.policy_recovery === true) {
    useUiStore.getState().pushToast(i18n.t('policy.recovery'))
    return
  }
  if (policyNeedsQuality(q)) {
    useUiStore.getState().pushToast(i18n.t(policyQualityMessage(q)))
    return
  }
  if (p.sub === 'tool_outcome_unknown') {
    useUiStore.getState().pushToast(i18n.t('cards.actionUnknownHint'))
    return
  }
  if (p.sub === 'acceptance_exception') {
    useUiStore.getState().pushToast(i18n.t('exceptions.hint'))
    return
  }
  if (q.kind === 'recovery' && p.sub === 'model_request' && (p.view as {state?: unknown} | undefined)?.state === 'needs_choice') {
    // MR-04: an ambiguous new instruction is the owner's explicit choice;
    // the approve shortcut must not pick "continue original" for them.
    useUiStore.getState().pushToast(i18n.t('modelRecovery.chooseHint'))
    return
  }
  if (q.kind === 'recovery' && p.sub === 'model_request' && (p.view as {state?: unknown} | undefined)?.state === 'retrying') {
    // test3 05/06 review: core rejects a second in-flight attempt; say so
    // instead of surfacing an already_running error toast.
    useUiStore.getState().pushToast(i18n.t('modelRecovery.alreadyRetrying'))
    return
  }
  // MR-08 / D01: the opening analysis re-runs through its lock-free command.
  if (q.kind === 'recovery' && p.sub === 'model_request' && p.purpose === 'opening_intake') return api.runOpeningIntake()
  if (q.kind === 'recovery' && p.sub === 'model_request') return api.retryModelRequest(q.id, String((p.view as {project_root?:unknown} | undefined)?.project_root ?? p.project_root ?? p.host_project_root ?? ''))
  if (q.kind === 'permission') return api.answerPermission(q.id, true)
  if (q.kind === 'stamp' && !p.proposal_id) return api.stamp()
  if (q.kind === 'stamp') return api.confirmProposal(q.id)
  if (q.kind === 'push') return api.confirmPush(q.id)
  if (q.kind === 'release') return api.confirmRelease(q.id)
  if (q.kind === 'worktree') return api.confirmWorktree(q.id)
  if (q.kind === 'escalation') return api.adjudicateFlag(q.id, true)
  if (q.kind === 'recovery' && p.sub === 'tool_action_ready') return api.resumeToolAction(String(p.action_id))
  if (q.kind === 'recovery') return api.recoverRun(String(p.run_id))
  if (q.kind === 'stall' && p.retry === true) return api.stallRetry(q.id)
  if (q.kind === 'install') return api.resolveInstall(q.id, true)
  if (q.kind === 'grant') return api.confirmGrant(q.id, true)
}

export async function rejectQuestion(q: PendingQuestion) {
  const p = q.payload
  if (p.sub === 'quality_revalidation') return api.cancelQualityRevalidation(q.id, String(p.project_root ?? ''))
  if (p.sub === 'design_direction') { useUiStore.getState().pushToast(i18n.t('design.chooseHint')); return }
  if (p.sub === 'tool_outcome_unknown') {
    useUiStore.getState().pushToast(i18n.t('cards.actionUnknownHint'))
    return
  }
  if (p.sub === 'acceptance_exception') return api.cancelAcceptanceException(q.id)
  if (q.kind === 'permission') return api.answerPermission(q.id, false)
  if (q.kind === 'stamp' && !p.proposal_id) return api.rejectStamp()
  if (q.kind === 'stamp') return api.rejectProposal(q.id, 'owner rejected')
  if (q.kind === 'push') return api.rejectPush(q.id)
  if (q.kind === 'release') return api.rejectRelease(q.id)
  if (q.kind === 'worktree') return api.rejectWorktree(q.id)
  if (q.kind === 'escalation') return api.adjudicateFlag(q.id, false)
  if (q.kind === 'install') return api.resolveInstall(q.id, false)
  if (q.kind === 'grant') return api.confirmGrant(q.id, false)
  if (q.kind === 'stall') return api.stallAck(q.id)
}

/// 逆建议驳回的留痕（ui-audit 票 07 / P2-15）：owner 驳回一张
/// judge=stamp 的提案卡时，reason 追加 `judge=<verdict>` 对照——
/// 日后翻 trace 能看出这次驳回是逆着机器建议来的。
/// judge=reject/needs_human 与顺向驳回不改 reason（无对照必要）。
export function rejectReasonWithJudge(reason: string, judgeVerdict: string | null): string {
  const base = reason.trim() || 'owner rejected'
  return judgeVerdict === 'stamp' ? `${base} · judge=${judgeVerdict}` : base
}

/** 卡 kind → 卡面标题 i18n key（kbd 目标提示 / sticky 迷你条共用） */
export function isActivationResumeCard(q: PendingQuestion): boolean {
  return q.kind === 'stall' && q.payload.source === 'activation_resume'
}

export function kindTitleKey(q: PendingQuestion): string {
  if (q.kind === 'recovery' && q.payload.sub === 'model_request') return q.payload.purpose === 'turn' ? 'modelRecovery.titleTurn' : 'modelRecovery.title'
  if (q.payload.sub === 'quality_revalidation') return 'quality.revalidationTitle'
  if (q.payload.sub === 'design_direction') return 'design.title'
  if (q.payload.sub === 'acceptance_exception') return 'exceptions.title'
  if (q.kind === 'stamp' && q.payload.proposal_id && q.payload.surface === 'pack_copy') return 'policy.title'
  if (q.kind === 'permission') return 'cards.ask'
  if (q.kind === 'stamp') return q.payload.proposal_id ? 'cards.proposalStamp' : 'cards.stageStamp'
  if (q.kind === 'push') return 'cards.push'
  if (q.kind === 'release') return 'cards.release'
  if (q.kind === 'worktree') return q.payload.operation === 'cleanup' ? 'cards.worktreeCleanup' : 'cards.worktreeCreate'
  if (q.kind === 'recovery') return q.payload.sub === 'tool_outcome_unknown' ? 'cards.actionUnknown' : 'cards.recovery'
  if (q.kind === 'stall') return isActivationResumeCard(q) ? 'cards.resumeWaiting' : 'cards.stall'
  if (q.kind === 'install') return 'cards.install'
  if (q.kind === 'grant') return 'cards.grant'
  if (q.kind === 'escalation') return 'cards.escalation'
  return q.kind
}

// ui-audit 票 02（P0-1/P0-2）：按键裁决的全部守卫集中在 handlePendingKey——
// 出处：全局监听挂在 window 且只看 e.target，设置页打开时 ⌘↵ 曾批准
// 看不见的顶卡；发布确认与「允许一次」同键，肌肉记忆即推送远端；
// 焦点停在批准钮上时还存在 Enter 激活 + 快捷键的双发路径。
// 守卫顺序：先认键（无关键静默放行）→ 无待决则放行 → 查作用域 →
// 拦 repeat/in-flight（双发）→ 拦 push（L3 不可逆不吃键盘批准键；原 publish，ADR 0093 改名）。
let adjudicating = false

export type KeyOutcome =
  | 'none'
  | 'blocked-scope'
  | 'repeat'
  | 'blocked-push'
  | 'blocked-release'
  | 'blocked-final'
  | 'blocked-stall'
  | 'blocked-unknown'
  | 'blocked-exception'
  | 'blocked-policy'
  | 'approved'
  | 'rejected'
  | 'failed'

export async function handlePendingKey(
  e: KeyboardEvent,
  sorted: PendingQuestion[],
): Promise<KeyOutcome> {
  const isApprove = matches(e, bindingFor('approve'))
  const isReject = matches(e, bindingFor('reject'))
  if (!isApprove && !isReject) return 'none'
  const q = sorted[0]
  if (!q) return 'none'
  // Enter 对聚焦按钮的默认 click 激活由 preventDefault 一并按住
  e.preventDefault()
  if (useUiStore.getState().modalScope !== 'workbench') {
    useUiStore.getState().pushToast(i18n.t('decisions.scopeBlocked'))
    return 'blocked-scope'
  }
  if (e.repeat || adjudicating) return 'repeat'
  if (q.payload.sub === 'tool_outcome_unknown') {
    useUiStore.getState().pushToast(i18n.t('cards.actionUnknownHint'))
    return 'blocked-unknown'
  }
  if (isApprove && q.payload.policy_recovery === true) {
    useUiStore.getState().pushToast(i18n.t('policy.recovery'))
    return 'blocked-policy'
  }
  if (isApprove && policyNeedsQuality(q)) {
    useUiStore.getState().pushToast(i18n.t(policyQualityMessage(q)))
    return 'blocked-policy'
  }
  if (isApprove && q.payload.sub === 'acceptance_exception') {
    useUiStore.getState().pushToast(i18n.t('exceptions.hint'))
    return 'blocked-exception'
  }
  if (q.payload.sub === 'quality_revalidation') {
    useUiStore.getState().pushToast(i18n.t('quality.revalidationHint'))
    return 'blocked-final'
  }
  if (isApprove && q.kind === 'push') {
    // L3 对外不可逆：推送不走键盘批准（ADR 0056 确认分级）
    useUiStore.getState().pushToast(i18n.t('decisions.pushNeedsClick'))
    return 'blocked-push'
  }
  if (isApprove && q.kind === 'release') {
    // owner-followups-1015 #15：发布通常不可逆、必问永不记忆——同推送，只认点「确认发布」按钮。
    useUiStore.getState().pushToast(i18n.t('decisions.releaseNeedsClick'))
    return 'blocked-release'
  }
  // 票 02：最终验收退回必须写阶段和修改意见，键盘驳回填不了这两项。
  if (isReject && q.kind === 'stamp' && !q.payload.proposal_id && q.payload.final_acceptance === true) {
    useUiStore.getState().pushToast(i18n.t('decisions.finalNeedsForm'))
    return 'blocked-final'
  }
  // stall-watch 票 02：失速卡只剩「知道了」时，批准键没有可批准的动作——
  // 不拿它顶替「知道了」，收场必须是负责人看清后按的那个键。
  if (isApprove && q.kind === 'stall' && q.payload.retry !== true) {
    useUiStore.getState().pushToast(i18n.t('decisions.stallNoRetry'))
    return 'blocked-stall'
  }
  adjudicating = true
  try {
    const decide = isApprove ? approveQuestion : rejectQuestion
    // 只提示不裁决的卡（视觉方向、模型恢复二选一…）不经「保存即消卡」：没有提交，卡不该闪一下。
    if (toastOnly(q, isApprove)) await decide(q)
    else await settleDecision(q.id, () => decide(q))
    return isApprove ? 'approved' : 'rejected'
  } catch (e) {
    // 票 13：只提示类卡（不经 settleDecision）的键盘裁决失败与点按钮同一出口——失败 toast（role=alert）+ 已登记原因码。
    // 曾经的写法：没有 catch，App 的 `void onKey(e)` 把拒绝吞成未处理 Promise，负责人无感知。
    toastError(e, { wrap: 'errors.actionFailed' })
    return 'failed'
  } finally {
    adjudicating = false
  }
}

function toastOnly(q: PendingQuestion, approve: boolean): boolean {
  const p = q.payload
  if (p.sub === 'design_direction') return true
  if (!approve) return false
  const state = (p.view as { state?: unknown } | undefined)?.state
  return q.kind === 'recovery' && p.sub === 'model_request' && (state === 'needs_choice' || state === 'retrying')
}

/** 全局待决快捷键：坞顶卡的 批准/驳回（mod+↵ / mod+⌫）。读键按下那一刻的 store，
 *  不靠渲染快照——「保存即消卡」途中顶卡已经换成下一张（票 04）。 */
const onPendingKey = (e: KeyboardEvent) => {
  const st = useUiStore.getState()
  return handlePendingKey(e, visiblePending(st.pending, st.hiddenPending))
}

export function usePendingKeys() {
  return { onKey: onPendingKey }
}
