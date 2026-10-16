// 票 16c：虚拟化时间线焦点保持（timelineFocus.ts 头注释写了曾经怎么坏的）。
import { afterEach, describe, expect, it } from 'vitest'
import { captureFocusAnchor, keepTimelineFocus } from './timelineFocus'

const frame = () => new Promise((r) => requestAnimationFrame(() => r(null)))

function setup() {
  const area = document.createElement('div')
  area.innerHTML = '<div data-virtuoso-scroller tabindex="0"><div data-timeline-row="e1"><button>open</button><button>copy</button></div><div data-timeline-row="e2"><button>x</button></div></div>'
  document.body.appendChild(area)
  const scroller = area.querySelector<HTMLElement>('[data-virtuoso-scroller]')!
  const off = keepTimelineFocus(area, () => scroller)
  return { area, scroller, off }
}

describe('keepTimelineFocus', () => {
  afterEach(() => { document.body.innerHTML = '' })

  it('anchors by row key and focusable index', () => {
    const { area, off } = setup()
    const copy = area.querySelectorAll('button')[1]
    expect(captureFocusAnchor(copy)).toEqual({ rowKey: 'e1', index: 1 })
    expect(captureFocusAnchor(area)).toBeNull()
    off()
  })

  it('puts focus back on the same control when the focused row is remounted', async () => {
    const { area, off } = setup()
    const copy = area.querySelectorAll<HTMLButtonElement>('button')[1]
    copy.focus()
    const fresh = area.querySelector<HTMLElement>('[data-timeline-row="e1"]')!.cloneNode(true) as HTMLElement
    area.querySelector('[data-timeline-row="e1"]')!.replaceWith(fresh)
    // 被移除的节点自己的事件冒不到 area——直接用 area 派发一份 target 为旧节点的 focusout
    const ev = new FocusEvent('focusout', { bubbles: true, relatedTarget: null })
    Object.defineProperty(ev, 'target', { value: copy })
    area.dispatchEvent(ev)
    ;(document.activeElement as HTMLElement | null)?.blur()
    await frame()
    expect(document.activeElement).toBe(fresh.querySelectorAll('button')[1])
    off()
  })

  it('falls back to the scroller when the row is no longer rendered', async () => {
    const { area, scroller, off } = setup()
    const x = area.querySelector<HTMLElement>('[data-timeline-row="e2"] button')!
    x.focus()
    area.querySelector('[data-timeline-row="e2"]')!.remove()
    const ev = new FocusEvent('focusout', { bubbles: true, relatedTarget: null })
    Object.defineProperty(ev, 'target', { value: x })
    area.dispatchEvent(ev)
    ;(document.activeElement as HTMLElement | null)?.blur()
    await frame()
    expect(document.activeElement).toBe(scroller)
    off()
  })

  it('does not steal focus when the element is still connected (click on blank space / window switch)', async () => {
    const { area, off } = setup()
    const open = area.querySelector<HTMLButtonElement>('button')!
    open.focus()
    open.blur()
    await frame()
    expect(document.activeElement).toBe(document.body)
    off()
  })
})

