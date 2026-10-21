// 画师提示（owner-followups-1015 票 19，规范 C1「画师消息」补设计，CONTEXT.md「画师」）。
// core `artist::IMAGE_MODEL_MISSING` System 事件的渲染：画师不是团队成员（不在 team 里），
// 所以名字取事件 payload.speaker、头像是首字色块；头像不可点（没有 Agent 详情可开）。
// 正文是界面文案（i18n），不是模型产物。按钮打开设置 › 模型服务并聚焦出图槽。
// 票 28：同一形制也渲染 `artist_prompt::PROMPT_FAILED`（画师的主对话模型写不出出图提示词）。
// 没有主对话模型时按钮聚焦画师的主对话槽；调用失败 / 输出不可用没有可去的设置，只说明、不给按钮。
import { useTranslation } from 'react-i18next'
import type { TimelineItem } from '../api'
import { fmtTime } from '../usage'
import { openSettingsSlot } from '../settingsSlotFocus'
import { Avatar as Face } from './ui/avatar'
import { Button } from './ui/button'

export const ARTIST_NOTICE_KIND = 'artist_image_model_missing'
export const ARTIST_PROMPT_FAILED_KIND = 'artist_prompt_failed'

type Notice = { state: string; text: string; slot: string | null; action: string }

function notice(p: Record<string, unknown>, t: (k: string, o?: Record<string, unknown>) => string): Notice {
  if (p.kind === ARTIST_PROMPT_FAILED_KIND) {
    const missing = p.reason === 'chat_model_missing'
    return {
      state: missing ? 'prompt_no_model' : 'prompt_failed',
      text: t(missing ? 'artist.promptNoModel' : 'artist.promptFailed'),
      slot: missing ? (typeof p.slot === 'string' ? p.slot : 'chat') : null,
      action: t('artist.bindChatModel'),
    }
  }
  const state = p.state === 'unsupported' ? 'unsupported' : 'unconfigured'
  return {
    state,
    text: t(`artist.${state}`, { model: String(p.model ?? '') }),
    slot: typeof p.slot === 'string' ? p.slot : 'image',
    action: t('artist.bindImageModel'),
  }
}

export function ArtistNotice({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const p = item.event.payload
  const speaker = String(p.speaker ?? '')
  const n = notice(p, t)
  const time = fmtTime(item.event.created_at)
  return (
    <div className="tl-msg" data-artist-notice={n.state}>
      <Face agentId={String(p.speaker_code ?? speaker)} name={speaker} size={30} />
      <div className="tl-grow">
        <div className="tl-who">
          <b>{speaker}</b>
          {time && <span className="t">{time}</span>}
        </div>
        <p className="msg-body">{n.text}</p>
        {n.slot && (
          <div className="mt-2">
            <Button size="xs" onClick={() => openSettingsSlot(n.slot!)}>{n.action}</Button>
          </div>
        )}
      </div>
    </div>
  )
}
