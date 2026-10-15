import type { TimelineItem } from './api'
import { DECISION_KINDS } from './timelineModel'

/** 底部留白带。输入框在时间线下面、不重叠，固定这段空隙，不跟视口成比例。 */
export const TIMELINE_TAIL_PX = 96

/**
 * 算「还在尾巴上」的带宽。
 * 2026-09 owner：回到底部要点两次。`followOutput` 只把视口钉在最后一行的底边，
 * 96px 留白留在视口外面；`atBottomThreshold=40` 又把「差一截」当成已经到底，
 * 按钮藏掉，跟随也不再补这一脚。带宽收成 4px，贴底必须把留白带算进视口。
 */
export const STICK_BAND_PX = 4

/** 视口底边对齐内容总高（含留白带）时的 scrollTop。不满一屏是 0，列表顶对齐。 */
export function tailScrollTop(scrollHeight: number, clientHeight: number): number {
  return Math.max(0, scrollHeight - clientHeight)
}

export function listOverflows(scrollHeight: number, clientHeight: number): boolean {
  return scrollHeight > clientHeight + 1
}

/** 松钉且内容高过窗口才出回到底部按钮。不满一屏本来就在顶上的「底」，按钮藏着。 */
export function showStickButton(pinned: boolean, overflows: boolean): boolean {
  return !pinned && overflows
}

export type StickScroll = {
  pinned: boolean
  /** 这次位移是我们写的 scrollTop，不是用户翻页。 */
  own: boolean
  /** 滚轮、拖滚动条、或紧跟其后的惯性。布局改高度不算。 */
  userMoved: boolean
  scrollTop: number
  previousScrollTop: number
  scrollHeight: number
  clientHeight: number
}

/**
 * 一次滚动之后还钉不钉。
 * 自己写的 scrollTop 不改钉，否则贴底那一下会被当成用户翻走。
 * 没有用户翻页时布局把 scrollTop 带偏也不松钉，下一笔还能写回真底。
 * 用户滚进带宽再钉上；往上离开带宽就松钉。钉着时内容长高、scrollTop 没往上，钉保持。
 */
export function pinAfterScroll(s: StickScroll): boolean {
  if (s.own) return s.pinned
  // 布局或节点轨的程序化滚动不改钉。否则刚松钉、视口还在尾巴上的那一帧会被立刻钉回去。
  if (!s.userMoved) return s.pinned
  const distance = s.scrollHeight - s.clientHeight - s.scrollTop
  if (distance <= STICK_BAND_PX) return true
  const delta = s.scrollTop - s.previousScrollTop
  if (delta < -0.5) return false
  return s.pinned && delta >= -0.5
}

// ---- 回到底部三态（ui-redesign 票 06，components C14，interaction §4，workbench#jump）----

/** 松钉以来的新内容。speakers：最近的发言者在前，去重，最多 3 个（头像叠放）。 */
export type FreshTally = { count: number; speakers: string[] }

const MAX_SPEAKERS = 3

/**
 * 计数口径：只算有意义的行——消息、决策、交付各算一条；工具调用、系统注记不计
 * （原型「避免『27 条新事件』这种噪音」）。负责人自己的发言不是「新消息」。
 */
export function freshTally(items: TimelineItem[], afterId: number): FreshTally {
  let count = 0
  const speakers: string[] = []
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i]
    const ev = it.event
    if (ev.id <= afterId) break
    if (ev.kind === 'owner_message' || it.message?.author === 'owner') continue
    if (it.message == null && !DECISION_KINDS.has(ev.kind) && ev.kind !== 'artifact_delivered') continue
    count++
    const who = it.message?.author ?? ev.agent_id
    if (who && !speakers.includes(who) && speakers.length < MAX_SPEAKERS) speakers.push(who)
  }
  return { count, speakers }
}

export type JumpFace =
  | { kind: 'round' }
  | { kind: 'fresh'; count: number; speakers: string[] }
  | { kind: 'replying'; agentId: string }

/** 三态：底部有流式输出时「某某 正在回复」优先；其次有新内容；否则只有箭头。 */
export function jumpFace(tally: FreshTally, replying: string | null): JumpFace {
  if (replying) return { kind: 'replying', agentId: replying }
  if (tally.count > 0) return { kind: 'fresh', count: tally.count, speakers: tally.speakers }
  return { kind: 'round' }
}
