import { memo } from 'react'
import { useTranslation } from 'react-i18next'
import type { PendingQuestion, TimelineItem } from '../api'
import { kindTitleKey } from '../decisions'
import { CHIP_TONE, rowChip } from '../managedView'
import { useManagedHistory } from '../useDelegation'
import { Icon } from './Icon'
import { Badge } from './ui/badge'
import './ProjectDelegation.css'

/** components D9 托管决定行（managed#running）：`system.managed_decision` 渲染成一行带作者的决定。
 * 作者固定「托管」（机器人图标，不冒充负责人）；一行写动作 + 卡种，下一行写理由；右侧结果 chip 只取
 * 宿主分类的 `ManagedRecord.result`（按 event_id 对上报告历史），历史里查不到（超出最近 N 条或尚未读到）
 * 就不出 chip——不从 outcome 文本猜结果（test3/23）。 */
export const ManagedRow = memo(function ManagedRow({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const { history } = useManagedHistory()
  const p = item.event.payload ?? {}
  const action = String(p.action ?? '')
  const kind = String(p.card_kind ?? '')
  const record = history?.find(r => r.event_id === item.event.id)
  const chip = record ? rowChip(record) : null
  const kindLabel = t(kindTitleKey({ id: String(p.card_id ?? ''), kind, agent_id: null, payload: {}, state: 'queued' } as unknown as PendingQuestion),
    { defaultValue: kind, who: '', tool: '', stage: '', name: '' })
  return <div className="mg-row" data-managed-row={item.event.id} data-managed-result={record?.result}>
    <div className="h">
      <span className="mg-av"><Icon name="bot" size={12} /></span>
      <b>{t('delegation.author')}</b>
      <span className="text-ink-2">{t(`delegation.action.${action}`, { defaultValue: action })}</span>
      {kind && <Badge shape="sq" className="min-w-0 truncate">{kindLabel}</Badge>}
      <span className="flex-1" />
      {chip && <Badge variant={CHIP_TONE[chip]}>{chip === 'executed' && <Icon name="check" size={12} />}{t(`delegation.rowChip.${chip}`)}</Badge>}
    </div>
    {p.reason ? <div className="r">{t('delegation.reasonLine', { reason: String(p.reason) })}
      {p.via ? <span className="text-ink-3"> · {t(`delegation.via.${String(p.via)}`, { defaultValue: String(p.via) })}</span> : null}</div> : null}
  </div>
})
