// 回到底部 JumpToLatest（ui-redesign 票 06，components C14，interaction §4，workbench#jump）。
// 对话列正中、贴在输入条 / 待决坞 / 托管细条 / 触顶横幅中最上者的上方 12px。三态：只有箭头 /
// 「N 条新消息」+ 头像叠放 + accent 点 / 「某某 正在回复」（像素格 + shimmer）。贴底语义不在这里：
// 是否出现（showStickButton）与点击（holdPin）由 Timeline 照旧决定，本组件只管样子与位置。
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'
import { useTranslation } from 'react-i18next'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'
import type { JumpFace } from '../timelineStick'
import { Icon } from './Icon'
import { Avatar, AvatarStack } from './ui/avatar'
import { PixelGrid, Shimmer } from './ui/spinner'

/** 与最上面那块底部表面的间距（C14「底边距输入条上沿 12」）。 */
export const JUMP_GAP_PX = 12
/** 对话列底部依次叠放的表面：触顶横幅、待决坞（展开 / 胶囊）、托管细条（票 08 挂 data-managed-strip）、输入条。 */
const BOTTOM_STACK = '[data-cap-banner], [data-dock], [data-managed-strip], .composer-wrap'

/**
 * 量出按钮的 bottom：时间线区域底边到底部叠放中最上那块上沿的距离 + 12。
 * 现布局里它们都在时间线下方的文档流里（距离 0 → 12）；浮在时间线上的表面（如以后的托管细条）
 * 只要带上 data-managed-strip 就会被让开，不用改这里。
 */
export function useJumpOffset(area: RefObject<HTMLElement | null>): number {
  const [bottom, setBottom] = useState(JUMP_GAP_PX)
  useLayoutEffect(() => {
    const el = area.current
    if (!el) return
    const scope = el.closest('[data-conversation]') ?? document
    const measure = () => {
      const floor = el.getBoundingClientRect().bottom
      const tops = [...scope.querySelectorAll<HTMLElement>(BOTTOM_STACK)]
        .map(node => node.getBoundingClientRect())
        .filter(box => box.height > 0 && box.top < floor)
        .map(box => box.top)
      const next = JUMP_GAP_PX + (tops.length ? floor - Math.min(...tops) : 0)
      setBottom(previous => previous === next ? previous : next)
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    scope.querySelectorAll<HTMLElement>(BOTTOM_STACK).forEach(node => observer.observe(node))
    return () => observer.disconnect()
  })
  return bottom
}

/** 计数播报节流：aria-live polite，≥2s 一次，不抢焦点（accessibility §3）。 */
export function useThrottledText(text: string, gapMs = 2000): string {
  const [shown, setShown] = useState(text)
  const last = useRef(0)
  useEffect(() => {
    if (text === shown) return
    const wait = Math.max(0, last.current + gapMs - Date.now())
    const timer = setTimeout(() => { last.current = Date.now(); setShown(text) }, wait)
    return () => clearTimeout(timer)
  }, [text, shown, gapMs])
  return shown
}

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
