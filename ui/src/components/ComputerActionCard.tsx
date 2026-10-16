import { useTranslation } from 'react-i18next'
import type { PendingQuestion } from '../api'
import { DecisionShell, KeyValues } from './DecisionShell'
import { PermissionActions } from './PermissionActions'
import { computerActionCall } from '../computerActionCall'

// 电脑动作确认卡（components D8「电脑动作」，control#action；票 ui-redesign 16B 补齐——此前走通用权限卡，
// 只写「QA 请求使用 computer_action」+ op=click x=… 原始参数）。
// computer_action 是安全网必问（permissions::is_safety_net，永不进记忆），所以只有「拒绝 · 执行这一次」，没有范围菜单 / 「记住」。
// 卡上只写宿主载荷里有的事实：应用 = app_id、动作 = 由 op 与参数拼出的调用式、目标 = element_id（有才出）。
// 原型的截图缩略与「理由」行宿主载荷没有（工具入参不带理由，快照不随卡下发），不造（reconciliation 票 16B 偏差）。

const VERBS = ['activate', 'click', 'type', 'key', 'scroll', 'drag'] as const
type Verb = typeof VERBS[number]

export function ComputerActionCard({ q, top, who }: { q: PendingQuestion; top: boolean; who: string }) {
  const { t } = useTranslation()
  const input = (q.payload.input ?? {}) as Record<string, unknown>
  const op = String(input.op ?? '')
  const verb = (VERBS as readonly string[]).includes(op) ? t(`dock.computerVerb.${op as Verb}`) : t('dock.computerVerbOther')
  const app = typeof input.app_id === 'string' && input.app_id ? input.app_id : ''
  const target = typeof input.element_id === 'string' && input.element_id ? input.element_id : ''
  const title = app ? t('dock.computerTitle', { who, app, verb }) : t('dock.computerTitleNoApp', { who, verb })
  return (
    <DecisionShell qid={q.id} cat="err" icon="pointer" title={title} sub={t('dock.computerSub')} note={null}
      actions={<PermissionActions key={q.id} q={q} top={top} noteText={t('dock.computerNote')} approveLabel={t('dock.computerRun')} />}>
      <KeyValues rows={[
        ...(app ? [[t('dock.kvApp'), app] as [string, string]] : []),
        [t('dock.kvAction'), computerActionCall(input), 'mono'],
        ...(target ? [[t('dock.kvTarget'), target, 'mono'] as [string, string, string]] : []),
      ]} />
    </DecisionShell>
  )
}
