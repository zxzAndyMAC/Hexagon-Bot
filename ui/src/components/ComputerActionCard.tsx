import { useTranslation } from 'react-i18next'
import type { PendingQuestion } from '../api'
import { DecisionShell, KeyValues } from './DecisionShell'
import { PermissionActions } from './PermissionActions'
import { computerActionCall } from '../computerActionCall'
import { DesktopToolDetails } from './DesktopToolDetails'

// 电脑动作确认卡（components D8「电脑动作」，control#action；票 ui-redesign 16B 补齐——此前走通用权限卡，
// 只写「QA 请求使用 computer_action」+ op=click x=… 原始参数）。
// computer_action 是安全网必问（permissions::is_safety_net，永不进记忆），所以只有「拒绝 · 执行这一次」，没有范围菜单 / 「记住」。
// 卡上只写宿主载荷里有的事实：应用 = app_id、动作 = 由 op 与参数拼出的调用式、目标 = element_id（有才出）。
// ui-redesign gap D6（2026-10-16）补齐原型的截图缩略与「理由」行：
// - 理由 = computer_action 入参 reason（模型写给负责人的一句话，tools/desktop.rs 剥离后才进原生 ABI）。只是展示——
//   安全网按工具名必问，理由缺失不挡卡、写得再有说服力也不免问；所以照原文显示、不加「已验证」之类修饰。
// - 缩略 = 载荷 screenshot（core desktop::actions::card_screenshot：动作所依据那次观察的证据引用，不是像素）。
//   查不到 / 已清除即 null → 不出缩略，只剩 kv（偏向「少一张图」，绝不因缺图拦卡）。像素仍走 desktop_screenshot
//   受限读取，点开复用截图证据的放大模态（components D19）。

const VERBS = ['activate', 'click', 'type', 'key', 'scroll', 'drag'] as const
type Verb = typeof VERBS[number]

export function ComputerActionCard({ q, top, who }: { q: PendingQuestion; top: boolean; who: string }) {
  const { t } = useTranslation()
  const input = (q.payload.input ?? {}) as Record<string, unknown>
  const op = String(input.op ?? '')
  const verb = (VERBS as readonly string[]).includes(op) ? t(`dock.computerVerb.${op as Verb}`) : t('dock.computerVerbOther')
  const app = typeof input.app_id === 'string' && input.app_id ? input.app_id : ''
  const target = typeof input.element_id === 'string' && input.element_id ? input.element_id : ''
  const reason = typeof input.reason === 'string' ? input.reason.trim() : ''
  const shot = q.payload.screenshot && typeof q.payload.screenshot === 'object' ? q.payload.screenshot as Record<string, unknown> : null
  const title = app ? t('dock.computerTitle', { who, app, verb }) : t('dock.computerTitleNoApp', { who, verb })
  return (
    <DecisionShell qid={q.id} cat="err" icon="pointer" title={title} sub={t('dock.computerSub')} note={null}
      actions={<PermissionActions key={q.id} q={q} top={top} noteText={t('dock.computerNote')} approveLabel={t('dock.computerRun')} />}>
      <div className={shot ? 'computer-action-body grid grid-cols-[200px_minmax(0,1fr)] items-start gap-3.5' : 'computer-action-body'}>
        {shot && <DesktopToolDetails output={{ result: shot }} variant="card" />}
        <KeyValues rows={[
          ...(app ? [[t('dock.kvApp'), app] as [string, string]] : []),
          [t('dock.kvAction'), computerActionCall(input), 'mono'],
          ...(target ? [[t('dock.kvTarget'), target, 'mono'] as [string, string, string]] : []),
          ...(reason ? [[t('dock.kvReason'), reason] as [string, string]] : []),
        ]} />
      </div>
    </DecisionShell>
  )
}
