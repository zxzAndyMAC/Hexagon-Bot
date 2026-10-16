import { useTranslation } from 'react-i18next'
import type { PendingQuestion } from '../api'
import { DecisionShell, KeyValues } from './DecisionShell'
import { PermissionActions } from './PermissionActions'
import { computerActionCall } from '../computerActionCall'
import { DesktopToolDetails } from './DesktopToolDetails'
import { cardApp, ringPoint } from '../computerTargetRing'

// 电脑动作确认卡（components D8「电脑动作」，control#action；票 ui-redesign 16B 补齐——此前走通用权限卡，
// 只写「QA 请求使用 computer_action」+ op=click x=… 原始参数）。
// computer_action 是安全网必问（permissions::is_safety_net，永不进记忆），所以只有「拒绝 · 执行这一次」，没有范围菜单 / 「记住」。
// 卡上只写宿主载荷里有的事实：应用 = 载荷 app（gap D8，见下）、动作 = 由 op 与参数拼出的调用式、目标 = element_id（有才出）。
// ui-redesign gap D6（2026-10-16）补齐原型的截图缩略与「理由」行：
// - 理由 = computer_action 入参 reason（模型写给负责人的一句话，tools/desktop.rs 剥离后才进原生 ABI）。只是展示——
//   安全网按工具名必问，理由缺失不挡卡、写得再有说服力也不免问；所以照原文显示、不加「已验证」之类修饰。
// - 缩略 = 载荷 screenshot（core desktop::actions::card_screenshot：动作所依据那次观察的证据引用，不是像素）。
//   查不到 / 已清除即 null → 不出缩略，只剩 kv（偏向「少一张图」，绝不因缺图拦卡）。像素仍走 desktop_screenshot
//   受限读取，点开复用截图证据的放大模态（components D19）。
// ui-redesign gap D8（2026-10-17）：
// - 应用 = 载荷 app（core desktop::actions::card_app：本地化应用名 + 窗口标题，查不到名回退 bundle id）。
//   D6 曾写 input.app_id——click 时根本没有，activate 时是 list_apps 的不透明回执 UUID，负责人看不懂，故弃用。
// - 缩略上的红色目标圈 = computerTargetRing.ringPoint；任何一项事实不可证就不画（画错位置比不画更坏）。

const VERBS = ['activate', 'click', 'type', 'key', 'scroll', 'drag'] as const
type Verb = typeof VERBS[number]

export function ComputerActionCard({ q, top, who }: { q: PendingQuestion; top: boolean; who: string }) {
  const { t } = useTranslation()
  const input = (q.payload.input ?? {}) as Record<string, unknown>
  const op = String(input.op ?? '')
  const verb = (VERBS as readonly string[]).includes(op) ? t(`dock.computerVerb.${op as Verb}`) : t('dock.computerVerbOther')
  const app = cardApp(q.payload.app)
  const target = typeof input.element_id === 'string' && input.element_id ? input.element_id : ''
  const reason = typeof input.reason === 'string' ? input.reason.trim() : ''
  const shot = q.payload.screenshot && typeof q.payload.screenshot === 'object' ? q.payload.screenshot as Record<string, unknown> : null
  const ring = shot ? ringPoint({ op, x: input.x, y: input.y, snapshotId: input.snapshot_id,
    shotSnapshotId: shot.snapshot_id, imageWidth: shot.image_width, imageHeight: shot.image_height }) : null
  const title = app ? t('dock.computerTitle', { who, app: app.name, verb }) : t('dock.computerTitleNoApp', { who, verb })
  return (
    <DecisionShell qid={q.id} cat="err" icon="pointer" title={title} sub={t('dock.computerSub')} note={null}
      actions={<PermissionActions key={q.id} q={q} top={top} noteText={t('dock.computerNote')} approveLabel={t('dock.computerRun')} />}>
      <div className={shot ? 'computer-action-body grid grid-cols-[200px_minmax(0,1fr)] items-start gap-3.5' : 'computer-action-body'}>
        {shot && <DesktopToolDetails output={{ result: shot }} variant="card" ring={ring} />}
        <KeyValues rows={[
          ...(app ? [[t('dock.kvApp'), app.title ? `${app.name} · ${app.title}` : app.name] as [string, string]] : []),
          [t('dock.kvAction'), computerActionCall(input), 'mono'],
          ...(target ? [[t('dock.kvTarget'), target, 'mono'] as [string, string, string]] : []),
          ...(reason ? [[t('dock.kvReason'), reason] as [string, string]] : []),
        ]} />
      </div>
    </DecisionShell>
  )
}
