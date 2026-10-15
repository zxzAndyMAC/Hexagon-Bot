import { describe, expect, it } from 'vitest'
import {
  TIMELINE_TAIL_PX,
  freshTally,
  jumpFace,
  listOverflows,
  pinAfterScroll,
  showStickButton,
  tailScrollTop,
} from './timelineStick'
import type { EventKind, TimelineItem } from './api'

const item = (id: number, kind: EventKind, opts: { agent?: string; author?: string; payload?: Record<string, unknown> } = {}): TimelineItem => ({
  event: { id, project_id: 'p', kind, agent_id: opts.agent ?? null, stage_run_id: null, payload: opts.payload ?? {}, created_at: '' },
  message: opts.author ? { id, author: opts.author, body: 'x', tokens: [], attachments: [], element_refs: [], created_at: '', thinking: '' } : null,
})

describe('回到底部计数口径（票 06）', () => {
  it('只算消息、决策、交付；工具调用与系统注记不计；负责人自己的发言不计', () => {
    const items = [
      item(1, 'message', { author: 'fe', agent: 'fe' }),
      item(2, 'message', { author: 'qa', agent: 'qa' }),
      item(3, 'tool_called', { agent: 'qa' }),
      item(4, 'tool_result', { agent: 'qa' }),
      item(5, 'system', { payload: { kind: 'note' } }),
      item(6, 'flag_submitted', { agent: 'qa' }),
      item(7, 'artifact_delivered', { agent: 'be' }),
      item(8, 'message', { author: 'owner' }),
      item(9, 'message', { author: 'fe', agent: 'fe' }),
    ]
    expect(freshTally(items, 1)).toEqual({ count: 4, speakers: ['fe', 'be', 'qa'] })
    expect(freshTally(items, 9)).toEqual({ count: 0, speakers: [] })
  })

  it('发言者头像最多 3 个，最近的在前', () => {
    const items = ['a', 'b', 'c', 'd'].map((who, i) => item(i + 1, 'message', { author: who, agent: who }))
    expect(freshTally(items, 0).speakers).toEqual(['d', 'c', 'b'])
  })

  it('三态：正在回复优先于新内容，否则只有箭头', () => {
    expect(jumpFace({ count: 0, speakers: [] }, null)).toEqual({ kind: 'round' })
    expect(jumpFace({ count: 3, speakers: ['fe'] }, null)).toEqual({ kind: 'fresh', count: 3, speakers: ['fe'] })
    expect(jumpFace({ count: 3, speakers: ['fe'] }, 'be')).toEqual({ kind: 'replying', agentId: 'be' })
  })
})

describe('时间线贴底', () => {
  it('贴底落点把留白带留在视口里，而不是停在最后一行的底边', () => {
    const contentEnd = 404
    const total = contentEnd + TIMELINE_TAIL_PX
    const view = 200
    const top = tailScrollTop(total, view)
    expect(top).toBe(300)
    expect(view - (contentEnd - top)).toBe(96)
  })

  it('不满一屏时落点是顶，内容没有高出窗口', () => {
    expect(tailScrollTop(80, 200)).toBe(0)
    expect(listOverflows(80, 200)).toBe(false)
  })

  it('往上离开尾巴就松钉', () => {
    expect(pinAfterScroll({
      pinned: true,
      own: false,
      userMoved: true,
      scrollTop: 200,
      previousScrollTop: 300,
      scrollHeight: 500,
      clientHeight: 200,
    })).toBe(false)
  })

  it('滚回尾巴再钉上', () => {
    expect(pinAfterScroll({
      pinned: false,
      own: false,
      userMoved: true,
      scrollTop: 296,
      previousScrollTop: 200,
      scrollHeight: 500,
      clientHeight: 200,
    })).toBe(true)
  })

  it('差过 4px 的上移不算还在尾巴上', () => {
    expect(pinAfterScroll({
      pinned: true,
      own: false,
      userMoved: true,
      scrollTop: 295,
      previousScrollTop: 300,
      scrollHeight: 500,
      clientHeight: 200,
    })).toBe(false)
  })

  it('贴底写入自己的 scrollTop 不把钉摘掉', () => {
    expect(pinAfterScroll({
      pinned: true,
      own: true,
      userMoved: false,
      scrollTop: 300,
      previousScrollTop: 40,
      scrollHeight: 500,
      clientHeight: 200,
    })).toBe(true)
  })

  it('往下翻但还没到尾巴，保持松钉', () => {
    expect(pinAfterScroll({
      pinned: false,
      own: false,
      userMoved: true,
      scrollTop: 120,
      previousScrollTop: 40,
      scrollHeight: 500,
      clientHeight: 200,
    })).toBe(false)
  })

  it('钉着时内容长高、视口没往上翻，钉保持，好让下一笔写到新的真底', () => {
    expect(pinAfterScroll({
      pinned: true,
      own: false,
      userMoved: false,
      scrollTop: 300,
      previousScrollTop: 300,
      scrollHeight: 600,
      clientHeight: 200,
    })).toBe(true)
  })

  it('没有用户翻页时，就算停在尾巴上也不重新钉', () => {
    expect(pinAfterScroll({
      pinned: false,
      own: false,
      userMoved: false,
      scrollTop: 296,
      previousScrollTop: 300,
      scrollHeight: 500,
      clientHeight: 200,
    })).toBe(false)
  })

  it('布局把视口往上带、用户没有翻页时，钉保持', () => {
    expect(pinAfterScroll({
      pinned: true,
      own: false,
      userMoved: false,
      scrollTop: 200,
      previousScrollTop: 300,
      scrollHeight: 500,
      clientHeight: 200,
    })).toBe(true)
  })

  it('回到底部按钮只在松钉且高过窗口时出现', () => {
    expect(showStickButton(false, true)).toBe(true)
    expect(showStickButton(true, true)).toBe(false)
    expect(showStickButton(false, false)).toBe(false)
  })
})
