import { describe, expect, it } from 'vitest'
import {
  TIMELINE_TAIL_PX,
  listOverflows,
  pinAfterScroll,
  showStickButton,
  tailScrollTop,
} from './timelineStick'

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
