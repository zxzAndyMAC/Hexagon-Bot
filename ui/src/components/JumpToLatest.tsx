// 回到底部 JumpToLatest（ui-redesign 票 06，components C14，interaction §4，workbench#jump）。
// 对话列正中、贴在输入条 / 待决坞 / 托管细条 / 触顶横幅中最上者的上方 12px。三态：只有箭头 /
// 「N 条新消息」+ 头像叠放 + accent 点 / 「某某 正在回复」（像素格 + shimmer）。贴底语义不在这里：
// 是否出现（showStickButton）与点击（holdPin）由 Timeline 照旧决定，本组件只管样子与位置。
import { useThrottledText } from '../useJumpOffset'
import { useTranslation } from 'react-i18next'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'
import type { JumpFace } from '../timelineStick'
import { Icon } from './Icon'
import { Avatar, AvatarStack } from './ui/avatar'
import { PixelGrid, Shimmer } from './ui/spinner'

function useRoleOf() {
  const team = useUiStore(s => s.team)
  return (id: string) => team.find(member => member.id === id)?.role ?? id
}

export function JumpToLatest({ face, bottom, onClick }: { face: JumpFace; bottom: number; onClick: () => void }) {
  const { t } = useTranslation()
  const roleOf = useRoleOf()
  const avatars = useUiStore(s => s.avatars)
  const binding = formatBinding(bindingFor('timelineLatest'))
  const label = face.kind === 'fresh' ? t('timeline.jumpFresh', { count: face.count })
    : face.kind === 'replying' ? t('timeline.jumpReplying', { name: roleOf(face.agentId) })
    : t('timeline.toBottom')
  return (
    <button type="button" className="jump hx-focus" data-jump={face.kind} style={{ bottom }}
      aria-label={label} title={binding ? `${t('timeline.toBottom')} ${binding}` : t('timeline.toBottom')} onClick={onClick}>
      {face.kind === 'fresh' && <>
        <span className="jump-nd" aria-hidden />
        <AvatarStack>
          {face.speakers.map(id => <Avatar key={id} agentId={id} name={roleOf(id)} size={20} src={avatars[id]} />)}
        </AvatarStack>
        <span aria-hidden>{t('timeline.newMessages', { count: face.count })}</span>
      </>}
      {face.kind === 'replying' && <>
        <PixelGrid />
        <Shimmer aria-hidden>{t('timeline.replying', { name: roleOf(face.agentId) })}</Shimmer>
      </>}
      <Icon name="arrow-down" size={face.kind === 'round' ? 16 : 14} className={face.kind === 'round' ? undefined : 'jump-ar'} />
    </button>
  )
}

/** 常驻的播报区：按钮出现与否都在，只读计数（「3 条新消息」）。 */
export function JumpAnnouncer({ face }: { face: JumpFace }) {
  const { t } = useTranslation()
  const text = useThrottledText(face.kind === 'fresh' ? t('timeline.newMessages', { count: face.count }) : '')
  return <span className="sr-only" aria-live="polite" data-jump-live>{text}</span>
}
