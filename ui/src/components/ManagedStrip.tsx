import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { kindTitleKey } from '../decisions'
import { latestDecisionEvent } from '../managedView'
import { TIMELINE_JUMP_EVENT } from '../timelineJump'
import { useDelegation, useManagedHistory } from '../useDelegation'
import { Button } from './ui/button'
import { PixelGrid } from './ui/spinner'
import './ProjectDelegation.css'

/** 细条上列出的事项名个数（其余只计数）。 */
const NAMED = 2

/** components D7 托管细条（managed#running，interaction §8）：托管生效（开启且未暂停）且宿主
 * 报告有正在处理的事项时，停在输入条上方，替待决坞占位——托管事项本就不进 owner_pending，
 * 所以坞此时通常为空；坞若仍有负责人自己的事项（例如 in_review 提案）照常显示，绝不藏起人工待决。
 * 「查看」跳到第一件事项最近的托管决定行（先找已载入的时间线，再找报告历史）。
 * `data-managed-strip`：票 06 的回到底部按钮按它定位（取输入条上方最上者）。 */
export function ManagedStrip() {
  const { t } = useTranslation()
  const { status, processingItems } = useDelegation()
  const { history } = useManagedHistory()
  const timeline = useUiStore(s => s.timeline)
  const active = !!status?.enabled && !status.paused && processingItems.length > 0
  if (!active) return null
  const names = processingItems.slice(0, NAMED).map(q => t(kindTitleKey(q), { defaultValue: q.kind, who: '', tool: '', stage: String(q.payload?.stage ?? ''), name: '' }))
  const target = processingItems.map(q => latestDecisionEvent(timeline, q.id)
    ?? history?.find(r => r.card_id === q.id)?.event_id ?? null).find(id => id != null) ?? null
  return <div className="mg-strip" data-managed-strip role="status">
    <PixelGrid />
    <span className="txt"><b className="font-medium">{t('delegation.stripTitle', { count: processingItems.length })}</b>
      <span className="text-ink-3"> · {names.join(' · ')}</span></span>
    {target != null && <Button size="xs" variant="ghost" title={t('delegation.stripViewHint')}
      onClick={() => window.dispatchEvent(new CustomEvent(TIMELINE_JUMP_EVENT, { detail: target }))}>
      {t('delegation.stripView')}
    </Button>}
  </div>
}
