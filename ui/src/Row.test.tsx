import { describe, expect, it, beforeEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { Row } from './components/Row'
import { Timeline } from './components/Timeline'
import { useUiStore } from './store'
import type { TimelineItem } from './gen/TimelineItem'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const key = (el: Element, k: string) =>
  el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }))

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

describe('Row 可达性原语（票 12）', () => {
  it('渲染 role + tabIndex=0；Enter 与 Space 激活 onClick，其余键不触发', async () => {
    const onClick = vi.fn()
    const { el, root } = await render(<Row role="option" selected onClick={onClick}>x</Row>)
    const row = el.querySelector('[role="option"]')!
    expect(row.getAttribute('tabindex')).toBe('0')
    expect(row.getAttribute('aria-selected')).toBe('true')
    key(row, 'a')
    expect(onClick).not.toHaveBeenCalled()
    key(row, 'Enter')
    key(row, ' ')
    expect(onClick).toHaveBeenCalledTimes(2)
    root.unmount()
  })
})

describe('NodeRail 键盘入口（票 12）', () => {
  const stamped = (id: number): TimelineItem => ({
    event: {
      id, project_id: 'p1', kind: 'stamped', agent_id: null, stage_run_id: null,
      payload: { stage: 'build' }, created_at: '2026-01-01T00:00:00Z',
    },
    message: null,
  }) as unknown as TimelineItem

  beforeEach(() => {
    useUiStore.setState({
      timeline: [stamped(1)], pending: [], streams: {}, streamDone: {},
      stages: [], nodeRailPulse: 0,
    })
  })

  it('focusNodeRail 递增脉冲 → 轨道展开且首条 option 聚焦；Enter 跳转后轨道收起', async () => {
    const { el, root } = await render(<Timeline />)
    // 折叠态：无 listbox
    expect(el.querySelector('[role="listbox"]')).toBeNull()
    await act(async () => { useUiStore.getState().focusNodeRail() })
    // rAF 聚焦
    await act(async () => { await new Promise((r) => setTimeout(r, 30)) })
    const list = el.querySelector('[role="listbox"]')
    expect(list).toBeTruthy()
    const opt = el.querySelector('[role="option"]')!
    expect(document.activeElement).toBe(opt)
    await act(async () => { key(opt, 'Enter') })
    // jump 后轨道收起（open=false → listbox 消失）
    expect(el.querySelector('[role="listbox"]')).toBeNull()
    root.unmount()
  })
})

describe('PracticeGround 隔离（票 20 / 方向卡 5）', () => {
  it('练习键命中只记数——不触真 answerPermission，且 stopPropagation 不冒泡', async () => {
    const { PracticeGround } = await import('./components/PracticeGround')
    const { api } = await import('./api')
    const spy = vi.spyOn(api, 'answerPermission')
    const { el, root } = await render(<PracticeGround />)
    const card = el.querySelector('[tabindex="0"]')!
    // 真绑定 = mod+Enter（happy-dom 非 mac → ctrlKey）
    let bubbled = false
    window.addEventListener('keydown', () => { bubbled = true }, { once: true })
    await act(async () => {
      card.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }))
    })
    expect(spy).not.toHaveBeenCalled()
    expect(bubbled).toBe(false) // stopPropagation 生效
    expect(el.textContent).toContain('✓')
    root.unmount()
  })

  it('非绑定键不记数不拦截', async () => {
    const { PracticeGround } = await import('./components/PracticeGround')
    const { el, root } = await render(<PracticeGround />)
    const card = el.querySelector('[tabindex="0"]')!
    let bubbled = false
    window.addEventListener('keydown', () => { bubbled = true }, { once: true })
    await act(async () => {
      card.dispatchEvent(new KeyboardEvent('keydown', { key: 'x', ctrlKey: true, bubbles: true }))
    })
    expect(bubbled).toBe(true)
    expect(el.textContent).not.toContain('✓')
    root.unmount()
  })
})
