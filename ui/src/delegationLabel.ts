import type { TFunction } from 'i18next'
import type { TimelineItem } from './api'
import { fmtTime } from './usage'

/** managed#running 系统行「14:20 · 你开启了托管 · 原模式「帮我批准」」（票 16B）。core 自此记 `change`
 *  （on / off / pause / resume）与项目保留的 `approval_mode`；更早的事件没有 change，回退通用「托管设置已变更」。 */
export function delegationLabel(t: TFunction, item: TimelineItem): string | null {
  const p = item.event.payload ?? {}
  const change = typeof p.change === 'string' ? p.change : ''
  if (!['on', 'off', 'pause', 'resume'].includes(change)) return null
  const parts = [fmtTime(item.event.created_at), t(`sys.delegation_${change}`)]
  if (change === 'on' && typeof p.approval_mode === 'string') {
    parts.push(t('sys.delegation_mode', { mode: t(`approvalMode.${p.approval_mode}`) }))
  }
  return parts.join(' · ')
}
