// 画师提示（owner-followups-1015 票 19，规范 C1「画师消息」补设计，CONTEXT.md「画师」）。
// core `artist::IMAGE_MODEL_MISSING` System 事件的渲染：画师不是团队成员（不在 team 里），
// 所以名字取事件 payload.speaker、头像是首字色块；头像不可点（没有 Agent 详情可开）。
// 正文是界面文案（i18n），不是模型产物。按钮打开设置 › 模型服务并聚焦出图槽。
import { useTranslation } from 'react-i18next'
import type { TimelineItem } from '../api'
import { fmtTime } from '../usage'
import { openSettingsSlot } from '../settingsSlotFocus'
import { Avatar as Face } from './ui/avatar'
import { Button } from './ui/button'

export const ARTIST_NOTICE_KIND = 'artist_image_model_missing'

export function ArtistNotice({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const p = item.event.payload
  const speaker = String(p.speaker ?? '')
  const state = p.state === 'unsupported' ? 'unsupported' : 'unconfigured'
  const slot = typeof p.slot === 'string' ? p.slot : 'image'
  const time = fmtTime(item.event.created_at)
  return (
    <div className="tl-msg" data-artist-notice={state}>
      <Face agentId={String(p.speaker_code ?? speaker)} name={speaker} size={30} />
      <div className="tl-grow">
        <div className="tl-who">
          <b>{speaker}</b>
          {time && <span className="t">{time}</span>}
        </div>
        <p className="msg-body">{t(`artist.${state}`, { model: String(p.model ?? '') })}</p>
        <div className="mt-2">
          <Button size="xs" onClick={() => openSettingsSlot(slot)}>{t('artist.bindImageModel')}</Button>
        </div>
      </div>
    </div>
  )
}
