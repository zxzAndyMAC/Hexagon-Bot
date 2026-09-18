// 待决卡的领域逻辑：严重度序 + 各 kind 的批准/驳回分发（ADR 0051）。
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { bindingFor, matches } from './keymap'

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

/** 全局待决快捷键：最高严重度卡的 批准/驳回（mod+↵ / mod+⌫） */
export function usePendingKeys() {
  const { pending } = useUiStore()
  const sorted = [...pending].sort((a, b) => severityOf(a) - severityOf(b))
  return {
    onKey: async (e: KeyboardEvent) => {
      const q = sorted[0]
      if (!q) return
      if (matches(e, bindingFor('approve'))) {
        e.preventDefault()
        await approveQuestion(q)
      } else if (matches(e, bindingFor('reject'))) {
        e.preventDefault()
        await rejectQuestion(q)
      }
    },
  }
}
