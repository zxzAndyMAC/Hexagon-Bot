// 待决卡的领域逻辑：严重度序 + 各 kind 的批准/驳回分发（ADR 0051）。
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { bindingFor, matches } from './keymap'
import i18n from './i18n'

// 严重度序：恢复/失速/发布/安装 > 阶段盖章 > 升级 > 权限 > 提案盖章
// 失速与恢复同档：两者都是「工作停了，不点不会自己动」（ADR 0074）。
const SEVERITY: Record<string, number> = {
  recovery: 0,
  stall: 0,
  publish: 0,
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
  if (q.kind === 'permission') return api.answerPermission(q.id, true)
  if (q.kind === 'stamp' && !p.proposal_id) return api.stamp()
  if (q.kind === 'stamp') return api.confirmProposal(q.id)
  if (q.kind === 'publish') return api.confirmPublish(q.id)
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
  if (q.kind === 'publish') return api.rejectPublish(q.id)
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
  if (q.payload.sub === 'quality_revalidation') return 'quality.revalidationTitle'
  if (q.payload.sub === 'design_direction') return 'design.title'
  if (q.payload.sub === 'acceptance_exception') return 'exceptions.title'
  if (q.kind === 'stamp' && q.payload.proposal_id && q.payload.surface === 'pack_copy') return 'policy.title'
  if (q.kind === 'permission') return 'cards.ask'
  if (q.kind === 'stamp') return q.payload.proposal_id ? 'cards.proposalStamp' : 'cards.stageStamp'
  if (q.kind === 'publish') return 'cards.publish'
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
// 拦 repeat/in-flight（双发）→ 拦 publish（L3 不可逆不吃键盘批准键）。
let adjudicating = false

export type KeyOutcome =
  | 'none'
  | 'blocked-scope'
  | 'repeat'
  | 'blocked-publish'
  | 'blocked-final'
  | 'blocked-stall'
  | 'blocked-unknown'
  | 'blocked-exception'
  | 'blocked-policy'
  | 'approved'
  | 'rejected'

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
  if (isApprove && q.kind === 'publish') {
    // L3 对外不可逆：发布不走键盘批准（ADR 0056 确认分级）
    useUiStore.getState().pushToast(i18n.t('decisions.publishNeedsClick'))
    return 'blocked-publish'
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
    if (isApprove) {
      await approveQuestion(q)
      return 'approved'
    }
    await rejectQuestion(q)
    return 'rejected'
  } finally {
    adjudicating = false
  }
}

/** 全局待决快捷键：最高严重度卡的 批准/驳回（mod+↵ / mod+⌫） */
export function usePendingKeys() {
  const { pending } = useUiStore()
  const sorted = [...pending].sort((a, b) => severityOf(a) - severityOf(b))
  return { onKey: (e: KeyboardEvent) => handlePendingKey(e, sorted) }
}
