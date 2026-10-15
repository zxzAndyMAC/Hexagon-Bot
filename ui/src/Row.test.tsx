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

  // ui-redesign 票 06（ADR 0088 节点轨节取代 0051，components C15）：「悬停展开 180px listbox」改为常显刻度列
  // （role=toolbar，每道刻度是 button + 可读名）；⌘J 聚焦首道刻度并出预览卡，原 listbox/option 断言随之改写。
  it('focusNodeRail 递增脉冲 → 首道刻度聚焦并出预览卡；↑↓ 移动；Esc 回输入条', async () => {
    const composer = document.createElement('div')
    composer.className = 'composer-wrap'
    const input = document.createElement('textarea')
    composer.appendChild(input)
    document.body.appendChild(composer)
    useUiStore.setState({ timeline: [stamped(1), stamped(2)] })
    const { el, root } = await render(<Timeline />)
    const rail = el.querySelector('[data-node-rail]')!
    expect(rail.getAttribute('role')).toBe('toolbar')
    expect(el.querySelector('[data-rail-card]')).toBeNull()
    await act(async () => { useUiStore.getState().focusNodeRail() })
    await act(async () => { await new Promise((r) => setTimeout(r, 30)) })
    const ticks = [...el.querySelectorAll<HTMLElement>('[data-tick]')]
    expect(ticks).toHaveLength(2)
    expect(document.activeElement).toBe(ticks[0])
    expect(ticks[0].getAttribute('aria-label')).toContain('build')
    expect(el.querySelector('[data-rail-card]')).not.toBeNull()
    await act(async () => { key(ticks[0], 'ArrowDown') })
    expect(document.activeElement).toBe(ticks[1])
    await act(async () => { key(ticks[1], 'Escape') })
    expect(document.activeElement).toBe(input)
    expect(el.querySelector('[data-rail-card]')).toBeNull()
    root.unmount()
    composer.remove()
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
    // ui-redesign 票 11（components E7）：命中反馈从「✓」字符换成 ok chip「⌘↵ 命中」
    expect(el.querySelector('[data-practice-hit]')).not.toBeNull()
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
    expect(el.querySelector('[data-practice-hit]')).toBeNull() // 票 11：同上，chip 取代 ✓
    root.unmount()
  })
})
