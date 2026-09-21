// 待决卡的领域逻辑：严重度序 + 各 kind 的批准/驳回分发（ADR 0051）。
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { bindingFor, matches } from './keymap'
import i18n from './i18n'

// 严重度序：恢复/发布/安装 > 阶段盖章 > 升级 > 权限 > 提案盖章
const SEVERITY: Record<string, number> = {
  recovery: 0,
  publish: 0,
  install: 0,
  stamp_stage: 1,
  escalation: 2,
  permission: 3,
  stamp_proposal: 4,
}

export function severityOf(q: PendingQuestion): number {
  if (q.kind === 'stamp') return q.payload.proposal_id ? SEVERITY.stamp_proposal : SEVERITY.stamp_stage
  return SEVERITY[q.kind] ?? 9
}

export async function approveQuestion(q: PendingQuestion) {
  const p = q.payload
  if (q.kind === 'permission') return api.answerPermission(q.id, true)
  if (q.kind === 'stamp' && !p.proposal_id) return api.stamp()
  if (q.kind === 'stamp') return api.confirmProposal(q.id)
  if (q.kind === 'publish') return api.confirmPublish(q.id)
  if (q.kind === 'escalation') return api.adjudicateFlag(q.id, true)
  if (q.kind === 'recovery') return api.recoverRun(String(p.run_id))
  if (q.kind === 'install') return api.resolveInstall(q.id, true)
}

export async function rejectQuestion(q: PendingQuestion) {
  const p = q.payload
  if (q.kind === 'permission') return api.answerPermission(q.id, false)
  if (q.kind === 'stamp' && !p.proposal_id) return api.rejectStamp()
  if (q.kind === 'stamp') return api.rejectProposal(q.id, 'owner rejected')
  if (q.kind === 'publish') return api.rejectPublish(q.id)
  if (q.kind === 'escalation') return api.adjudicateFlag(q.id, false)
  if (q.kind === 'install') return api.resolveInstall(q.id, false)
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
export function kindTitleKey(q: PendingQuestion): string {
  if (q.kind === 'permission') return 'cards.ask'
  if (q.kind === 'stamp') return q.payload.proposal_id ? 'cards.proposalStamp' : 'cards.stageStamp'
  if (q.kind === 'publish') return 'cards.publish'
  if (q.kind === 'recovery') return 'cards.recovery'
  if (q.kind === 'install') return 'cards.install'
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
  if (isApprove && q.kind === 'publish') {
    // L3 对外不可逆：发布不走键盘批准（ADR 0056 确认分级）
    useUiStore.getState().pushToast(i18n.t('decisions.publishNeedsClick'))
    return 'blocked-publish'
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
