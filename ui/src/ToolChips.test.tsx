// beautiful-ui 票 02：ToolGroupRow chip 化行为面——
// 调用对计数、逐行 chip、行展开明细、文件片开 tab、在途蓝点。
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { ToolGroupRow } from './components/Timeline'
import { useUiStore } from './store'
import type { TimelineItem } from './api'
import type { EventKind } from './gen/EventKind'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  await act(async () => {})
  return { el, root }
}

const ev = (id: number, kind: EventKind, payload: Record<string, unknown>): TimelineItem => ({
  event: {
    id, project_id: 'p1', kind, agent_id: 'a1',
    stage_run_id: null, payload, created_at: '',
  },
  message: null,
})

const CALLS: TimelineItem[] = [
  ev(1, 'tool_called', { tool: 'fs_read', input: { path: 'docs/brief.md' } }),
  ev(2, 'tool_result', { tool: 'fs_read', ok: true }),
  ev(3, 'tool_called', { tool: 'fs_write', input: { path: 'specs/prd.md', bytes: 2048 } }),
  ev(4, 'tool_result', { tool: 'fs_write', ok: false }),
  // 在途：无 result 配对（exec-cards 票 02：bash input 真字段是 cmd）
  ev(5, 'tool_called', { tool: 'bash', input: { cmd: 'cargo test' }, seq: 'r0:i2' }),
]

beforeEach(() => {
  document.body.innerHTML = ''
  useUiStore.setState({
    team: [{ id: 'a1', role: '后端开发', model_slot: null, status: 'active', avatar_hash: null }],
    tabs: [{ id: 'timeline', kind: 'timeline', title: 'timeline' }] as never,
    activeTab: 'timeline',
  })
})

describe('ToolGroupRow（beautiful-ui 票 02）', () => {
  it('折叠头按调用对计数，不按事件条数', async () => {
    const { el, root } = await render(<ToolGroupRow items={CALLS} expanded={false} idx={0} onToggle={() => {}} />)
    // 3 对调用（5 条事件）——旧版会显示 5
    expect(el.textContent).toContain('3')
    expect(el.textContent).not.toContain('5')
    expect(el.querySelectorAll('.tchip-row').length).toBe(0)
    root.unmount()
  })

  it('展开渲染逐行 chip：定名 + 参数片 + 结果态', async () => {
    const { el, root } = await render(<ToolGroupRow items={CALLS} expanded idx={0} onToggle={() => {}} />)
    // exec-cards 票 02（spec D2 分层）：fs_write/bash 升 .exec-card，
    // 轻量读系（fs_read）保持 .tchip-row——本组 fixture 只剩一条 chip。
    const rows = el.querySelectorAll('.tchip-row')
    const cards = el.querySelectorAll('.exec-card')
    expect(rows.length).toBe(1)
    expect(cards.length).toBe(2)
    expect(el.textContent).toContain('Read file') // agent.stepRead
    expect(el.textContent).toContain('docs/brief.md')
    expect(el.textContent).toContain('cargo test')
    expect(el.querySelectorAll('.chip.ok').length).toBe(1)
    expect(el.querySelectorAll('.chip.err').length).toBe(1)
    // 票 05：在途调用=SpinnerRing 运行环（升卡后挂在 exec-head 同一状态机），落定行无环
    expect(cards[1].querySelector('.spinner-ring')).not.toBeNull() // bash 在途
    expect(rows[0].querySelector('.spinner-ring')).toBeNull()
    root.unmount()
  })

  it('票 05：在途环随 result 到达翻面成 ok/err 徽标', async () => {
    const { el, root } = await render(<ToolGroupRow items={CALLS} expanded idx={0} onToggle={() => {}} />)
    // 票 02 起 bash 是 exec-card——在途环在卡头行同一状态机位。
    const inFlight = el.querySelectorAll('.exec-card')[1]
    expect(inFlight.querySelector('.spinner-ring')).not.toBeNull()
    const settled = [...CALLS, ev(6, 'tool_result', { tool: 'bash', ok: true })]
    await act(async () => { root.render(<ToolGroupRow items={settled} expanded idx={0} onToggle={() => {}} />) })
    const last = el.querySelectorAll('.exec-card')[1]
    expect(last.querySelector('.spinner-ring')).toBeNull()
    expect(last.querySelector('.chip.ok')).not.toBeNull()
    root.unmount()
  })

  it('行点击展开 input/result 明细', async () => {
    const { el, root } = await render(<ToolGroupRow items={CALLS} expanded idx={0} onToggle={() => {}} />)
    const first = el.querySelector('.tchip-row')!
    expect(el.querySelector('.tchip-detail')).toBeNull()
    await act(async () => { first.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const detail = el.querySelector('.tchip-detail')
    expect(detail).not.toBeNull()
    expect(detail!.textContent).toContain('fs_read')
    expect(detail!.textContent).toContain('brief.md')
    root.unmount()
  })

  it('文件片点开产物 tab（去重）', async () => {
    const { el, root } = await render(<ToolGroupRow items={CALLS} expanded idx={0} onToggle={() => {}} />)
    const chips = el.querySelectorAll('.tchip-files .chip-btn')
    expect(chips.length).toBe(2) // brief.md + prd.md 去重；bash 无 path
    await act(async () => { chips[1].dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const tabs = useUiStore.getState().tabs
    expect(tabs.some((t) => t.id === 'art:specs/prd.md')).toBe(true)
    root.unmount()
  })
})
