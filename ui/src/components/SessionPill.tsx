// 头部「会话」胶囊（components C12a「改为头部胶囊」，owner-followups-1015 #11；2026-10-17 负责人裁决 followups 第 2 / 9 项）。
// 会话卡曾常驻钉在时间线顶部，1440×900 下把钉住的归来摘要压到约 170px，「需要你处理」被裁（reconciliation 偏差 4）。
// 现在：会话异常时头部 B5 状态槽（全宽时 B7 页签栏状态槽）出一枚胶囊，色随最严重项，点开 A16 弹层 = 原卡内容；健康时不出现。
// 同一时刻只挂一份（Header 非全宽 / CanvasTabs 全宽）：本组件注册会话键位，挂两份会一键执行两次（同 ManagedPill）。
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { useUiStore } from '../store'
import { alertTransition, sessionPillView, type PillReason, type PillTone } from '../sessionPill'
import type { SessionStatus } from '../gen/SessionStatus'
import { SessionLaterPicker } from './SessionLater'
import { SessionRecoveryCard, label as fallbackLabel, useSessionActions, useSessionKeys } from './SessionRecoveryStrip'
import { badgeVariants } from '@/components/ui/badge'
import { Dot } from '@/components/ui/dot'
import { Popover, PopoverAnchor, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'

const DOT_TONE: Record<PillTone, 'err' | 'warn' | 'info' | 'on' | 'neutral'> = {
  err: 'err', warn: 'warn', info: 'info', ok: 'on', neutral: 'neutral',
}

function reasonText(t: TFunction, reason: PillReason): string {
  switch (reason.kind) {
    case 'error': return t('session.pillError')
    case 'status': return t(`session.status_${reason.status}`)
    case 'expired': return t('session.pillExpired')
    case 'scheduled': return t('session.pillScheduled')
    case 'capability': {
      const kind = fallbackLabel(t, `session.capability_${reason.group.kind}`, 'session.capability_other')
      return t('session.capGroup', { kind, healthy: reason.group.healthy, total: reason.group.total })
    }
  }
}

/** 转入 failed / reconnecting：读屏播报一次 + 胶囊单次脉冲（减少动效时全局规则关掉动画）。不自动弹开、不钉顶。 */
function useAlertOnTransition(status: SessionStatus | null) {
  const { t } = useTranslation()
  // undefined = 本实例还没见过状态：挂载时已是 failed 不算「转入」，免得画布全宽切换（胶囊换挂点重挂）时重播
  const prev = useRef<SessionStatus | null | undefined>(undefined)
  const [announce, setAnnounce] = useState('')
  const [pulse, setPulse] = useState(0)
  useEffect(() => {
    const hit = alertTransition(prev.current, status)
    prev.current = status
    if (!hit) return
    setAnnounce(t(`session.announce_${hit}`))
    setPulse((n) => n + 1)
  }, [status, t])
  return { announce, pulse, clearPulse: () => setPulse(0) }
}

/** 胶囊本体：色随最严重项；转入异常时单次脉冲，动画结束清掉。 */
function PillTrigger({ tone, pulse, label, onPulseEnd }: {
  tone: PillTone
  pulse: boolean
  label: string
  onPulseEnd: () => void
}) {
  const { t } = useTranslation()
  return (
    <PopoverTrigger asChild>
      <button type="button" aria-haspopup="dialog" data-session-pill={tone}
        data-session-pulse={pulse || undefined} title={t('session.ariaLabel')}
        onAnimationEnd={(e) => { if (e.target === e.currentTarget) onPulseEnd() }}
        className={cn(badgeVariants({ variant: tone }), 'hx-press hx-focus max-w-[260px] cursor-pointer', pulse && 'animate-hx-pulse-once')}>
        <Dot tone={DOT_TONE[tone]} />
        <span className="truncate">{label}</span>
      </button>
    </PopoverTrigger>
  )
}

export function SessionPill() {
  const { t } = useTranslation()
  const actions = useSessionActions()
  const laterOpen = useUiStore((s) => s.sessionLaterOpen)
  const setLaterOpen = useUiStore((s) => s.setSessionLaterOpen)
  const [cardOpen, setCardOpen] = useState(false)
  const [treeOpen, setTreeOpen] = useState(false)
  const view = sessionPillView(actions.summary, actions.error)
  const { announce, pulse, clearPulse } = useAlertOnTransition(actions.session?.status ?? null)
  useSessionKeys(actions, {
    openLater: () => { setCardOpen(false); setLaterOpen(true) },
    // 健康时胶囊不出现，创建工作树键位仍要有落点：弹开卡片（胶囊随卡片打开临时出现）再开路径弹层
    openTree: () => { setCardOpen(true); setTreeOpen(true) },
  })
  // 预约后不再可约（已有预约 / 会话结束）时收起时间弹层，免得停在一个点不了的「预约」上
  useEffect(() => { if (laterOpen && actions.laterBlocked) setLaterOpen(false) }, [laterOpen, actions.laterBlocked, setLaterOpen])
  const shown = view.kind === 'shown' || cardOpen
  const tone: PillTone = view.kind === 'shown' ? view.tone : 'neutral'
  const text = view.kind === 'shown' ? reasonText(t, view.reason) : t(`session.status_${actions.session?.status ?? 'running'}`)
  const pillLabel = t('session.pillLabel', { state: text })
  return (
    <>
      <Popover open={laterOpen} onOpenChange={setLaterOpen}>
        <PopoverAnchor asChild>
          <span className="inline-flex flex-none" data-session-slot>
            {shown && (
              <Popover open={cardOpen} onOpenChange={(open) => { setCardOpen(open); if (!open) setTreeOpen(false) }}>
                <PillTrigger tone={tone} pulse={pulse > 0} label={pillLabel} onPulseEnd={clearPulse} />
                <PopoverContent align="start" aria-label={t('session.ariaLabel')}
                  className="w-[460px] max-w-[calc(100vw-48px)] max-h-[calc(100vh-80px)] overflow-y-auto p-0">
                  <SessionRecoveryCard actions={actions} treeOpen={treeOpen} onTreeOpen={setTreeOpen} />
                </PopoverContent>
              </Popover>
            )}
          </span>
        </PopoverAnchor>
        {/* 入口在 ⋯ 菜单 / ⌘K：它们关闭时把焦点还给各自触发处，radix 视为「焦点移出」会立刻关掉刚开的弹层
            （#11 vitest 复现：开→关同一拍）。这里不因焦点移出关闭，点外面 / Esc 照常关。 */}
        <PopoverContent align="start" aria-label={t('session.scheduleTitle')} data-session-later
          onFocusOutside={(e) => e.preventDefault()}>
          {laterOpen && <SessionLaterPicker onSchedule={(at) => { setLaterOpen(false); void actions.scheduleAt(at) }} />}
        </PopoverContent>
      </Popover>
      {/* 播报区常驻（aria-live 区域要先在 DOM 里，后改文字才会被读）；只在转入 failed / reconnecting 时写一次 */}
      <span className="sr-only" role="status" aria-live="polite" data-session-announce>{announce}</span>
    </>
  )
}
