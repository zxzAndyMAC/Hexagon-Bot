// 节点轨 NodeRail（ui-redesign 票 06，components C15，interaction §5，accessibility §4，workbench#rail）。
// 对话右缘一列刻度，取代「图标列 + 悬停展开 180px 列表」（ADR 0051 节点轨节由 0088 取代）。
// 刻度等长 16（2026-10-26 起，原长度 = 层级），阶段里程碑 green，视口内提亮（in / cur），悬停或键盘聚焦出预览卡，点击跳转并闪烁目标行。
// 键盘：⌘J（nodeRail）聚焦首道刻度，↑↓ 逐道移动并显示预览卡，↵ 跳转，Esc 回输入条。
// 窗口过窄或画布挤占时 data-compact 收起不可见，键盘聚焦进来时照常显示（⌘J 仍可用）。
// 鱼眼（票 27）：指针在轨上移动时按距离放大附近刻度，逻辑与取值出处见 railFisheye.ts。
import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'
import type { NodeMark } from '../timelineModel'
import { Icon } from './Icon'
import { Avatar } from './ui/avatar'
import { Badge } from './ui/badge'
import { Kbd } from './ui/kbd'
import { useRailFisheye } from './railFisheye'

type Highlight = 'cur' | 'in' | undefined

function clock(at: string | undefined, locale: string): string {
  if (!at) return ''
  const date = new Date(at)
  if (Number.isNaN(date.getTime())) return ''
  return date.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit', hour12: false })
}

function titleOf(mark: NodeMark, t: TFunction): string {
  if (mark.type === 'pending') return t('rail.pendingTitle', { count: Number(mark.label) })
  if (mark.type === 'events') return t('rail.eventsTitle', { count: mark.merged?.length ?? Number(mark.label) })
  return mark.label
}

function whoOf(mark: NodeMark, t: TFunction, roleOf: (id: string) => string): string {
  if (mark.type === 'owner') return t('rail.you')
  return mark.agentId ? roleOf(mark.agentId) : ''
}

const STATUS = { warn: ['warn', 'rail.statusPending'], err: ['err', 'rail.statusFailed'], mg: ['info', 'rail.statusManaged'] } as const

/** 每道刻度的可读名：「产物交付 src/TodoList.tsx，前端，14:02」（accessibility §4）。 */
function tickName(mark: NodeMark, t: TFunction, roleOf: (id: string) => string, locale: string): string {
  const head = t('rail.name', { type: t(`rail.type.${mark.type}`), title: titleOf(mark, t) })
  const status = mark.tone ? t(STATUS[mark.tone][1]) : ''
  return [head, whoOf(mark, t, roleOf), clock(mark.at, locale), mark.type === 'pending' ? '' : status]
    .filter(Boolean).join(t('rail.sep'))
}

function PreviewCard({ mark, top, roleOf }: { mark: NodeMark; top: number; roleOf: (id: string) => string }) {
  const { t, i18n } = useTranslation()
  const avatars = useUiStore(s => s.avatars)
  const who = whoOf(mark, t, roleOf)
  const when = clock(mark.at, i18n.language)
  const raw = mark.merged ? mark.merged.map(m => m.label).join(' · ') : mark.summary
  // 一行的负责人原话已经是标题，不再重复一遍摘要
  const summary = raw?.trim() === mark.label.trim() ? undefined : raw
  return (
    <div className="rail-card" style={{ top }} aria-hidden data-rail-card>
      <div className="rail-card-k">
        <Icon name={mark.icon} size={12} />
        {t(`rail.type.${mark.type}`)}
        <span className="rail-sp" />
        <Kbd>↵ {t('rail.jump')}</Kbd>
      </div>
      <div className="rail-card-t" data-mono={mark.type === 'deliver' || undefined}>{titleOf(mark, t)}</div>
      {summary && <div className="rail-card-p">{summary}</div>}
      {(who || when || mark.tone) && (
        <div className="rail-card-f">
          {mark.agentId && mark.type !== 'owner' && <Avatar agentId={mark.agentId} name={who} size={18} src={avatars[mark.agentId]} />}
          {mark.type === 'owner' && <Avatar name={who} owner size={18} />}
          {[who, when].filter(Boolean).join(' · ')}
          <span className="rail-sp" />
          {mark.tone && <Badge variant={STATUS[mark.tone][0]}>{t(STATUS[mark.tone][1])}</Badge>}
        </div>
      )}
    </div>
  )
}

function focusComposer() {
  document.querySelector<HTMLElement>('.composer-wrap textarea')?.focus()
}

export function NodeRail({ marks, highlights, compact, onJump }: {
  marks: NodeMark[]
  highlights: Highlight[]
  /** 窗口过窄或画布挤占：不可见，键盘聚焦时显示。 */
  compact: boolean
  onJump: (mark: NodeMark) => void
}) {
  const { t, i18n } = useTranslation()
  const team = useUiStore(s => s.team)
  const roleOf = (id: string) => team.find(member => member.id === id)?.role ?? id
  const railRef = useRef<HTMLDivElement>(null)
  const [active, setActive] = useState<number | null>(null)
  const [cardTop, setCardTop] = useState(0)
  const [focusIndex, setFocusIndex] = useState(0)
  const fisheye = useRailFisheye(railRef, marks.length)
  const ticks = () => [...(railRef.current?.querySelectorAll<HTMLButtonElement>('[data-tick]') ?? [])]

  const show = (index: number) => {
    const tick = ticks()[index]
    setActive(index)
    // 预览卡顶边对齐刻度，往上让出类型行（原型 rail-card 与刻度同高）
    if (tick) setCardTop(Math.max(0, tick.offsetTop - 24))
  }

  // ⌘J：pulse 递增即聚焦首道刻度（App 的 nodeRail 键 → store.focusNodeRail）。
  const pulse = useUiStore(s => s.nodeRailPulse)
  const seenPulse = useRef(pulse)
  useEffect(() => {
    if (pulse === seenPulse.current) return
    seenPulse.current = pulse
    requestAnimationFrame(() => ticks()[0]?.focus())
  }, [pulse])

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const list = ticks()
    const index = list.indexOf(document.activeElement as HTMLButtonElement)
    if (index < 0) return
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const next = Math.min(list.length - 1, Math.max(0, index + (event.key === 'ArrowDown' ? 1 : -1)))
      list[next].focus()
    } else if (event.key === 'Escape') {
      // Esc 层级（interaction §12）：节点轨 → 回输入条；不再冒泡去收别的层。
      event.preventDefault()
      event.stopPropagation()
      setActive(null)
      focusComposer()
    }
  }

  if (!marks.length) return null
  const binding = formatBinding(bindingFor('nodeRail'))
  const safeFocus = Math.min(focusIndex, marks.length - 1)
  return (
    <div ref={railRef} className="rail" data-node-rail data-compact={compact || undefined}
      role="toolbar" aria-orientation="vertical" aria-label={binding ? `${t('rail.label')} ${binding}` : t('rail.label')}
      onMouseLeave={() => setActive(null)} onKeyDown={onKeyDown} {...fisheye}
      onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setActive(null) }}>
      {marks.map((mark, index) => (
        <button key={mark.eventId ?? `${mark.type}-${index}`} type="button" data-tick className="tk"
          data-tier={mark.tier} data-tone={mark.tone} data-hl={highlights[index]}
          // owner-followups-1015 #08：消息右键「在节点轨中定位」按事件 id 找刻度（合并刻度列出全部）。
          data-tick-event={mark.merged ? undefined : mark.eventId}
          data-tick-events={mark.merged?.map(m => m.eventId).filter(id => id != null).join(' ')}
          tabIndex={index === safeFocus ? 0 : -1}
          aria-label={tickName(mark, t, roleOf, i18n.language)}
          onMouseEnter={() => show(index)}
          onFocus={() => { setFocusIndex(index); show(index) }}
          onClick={() => { setActive(null); onJump(mark) }} />
      ))}
      {active != null && marks[active] && <PreviewCard mark={marks[active]} top={cardTop} roleOf={roleOf} />}
    </div>
  )
}
