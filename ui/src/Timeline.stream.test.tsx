import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { LiveReply, StatusLine, ThinkingRow } from './components/Timeline'
import { THINKING_COLLAPSE_AT } from './timelineModel'
import { useUiStore } from './store'
import type { TurnDelta } from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  await act(async () => {})
  return { el, root }
}

const delta = (over: Partial<TurnDelta>): TurnDelta => ({
  agent_id: 'a1',
  stage_run_id: null,
  call: 0,
  reset: false,
  done: false,
  text: '',
  thinking: '',
  ...over,
})

function Probe() {
  const text = useUiStore((s) => s.streams.a1?.[0] ?? '')
  const thinking = useUiStore((s) => s.thinkings.a1?.[0] ?? '')
  const done = useUiStore((s) => s.streamDone.a1)
  if (!text && !thinking) return null
  return <LiveReply role="后端开发" text={text} thinking={thinking} generating={!done} />
}

describe('时间线增量与思考（hands-free 票 06）', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    useUiStore.setState({
      streams: {}, thinkings: {}, streamDone: {}, timeline: [],
      stages: [], team: [], pending: [],
    })
  })

  it('增量到达就追加在同一条进行中的回复上，不另起多条', async () => {
    const { el, root } = await render(<Probe />)
    expect(el.querySelector('[data-live-reply]')).toBeNull()
    await act(async () => { useUiStore.getState().applyDelta(delta({ text: '你' })) })
    expect(el.querySelectorAll('[data-live-reply]')).toHaveLength(1)
    expect(el.textContent).toContain('你')
    expect(el.querySelector('[data-generating]')).toBeTruthy()
    await act(async () => { useUiStore.getState().applyDelta(delta({ text: '好' })) })
    expect(el.querySelectorAll('[data-live-reply]')).toHaveLength(1)
    expect(el.textContent).toContain('你好')
    expect(el.textContent).not.toContain('你好好')
    root.unmount()
  })

  it('过长思考收成一行，点开全文，再点收回省略', async () => {
    const full = `${'推'.repeat(THINKING_COLLAPSE_AT)}尾巴`
    const { el, root } = await render(<ThinkingRow text={full} />)
    const row = el.querySelector('[data-thinking]')!
    expect(row.getAttribute('data-open')).toBe('0')
    expect(row.textContent).toContain('…')
    expect(row.textContent).not.toContain('尾巴')
    await act(async () => { row.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(row.getAttribute('data-open')).toBe('1')
    expect(row.textContent).toContain('尾巴')
    expect(row.textContent).not.toContain('…')
    await act(async () => { row.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(row.getAttribute('data-open')).toBe('0')
    expect(row.textContent).not.toContain('尾巴')
    expect(row.textContent).toContain('…')
    root.unmount()
  })

  it('没有思考文本时不出现思考区', async () => {
    const { el, root } = await render(
      <LiveReply role="后端开发" text="只有正文" thinking="" generating />,
    )
    expect(el.querySelector('[data-thinking]')).toBeNull()
    expect(el.textContent).toContain('只有正文')
    expect(el.querySelector('[data-generating]')).toBeTruthy()
    root.unmount()
    const blank = await render(<ThinkingRow text="   " />)
    expect(blank.el.querySelector('[data-thinking]')).toBeNull()
    blank.root.unmount()
  })

  it('状态行显示阶段、角色和工具，不采用模型计划里的名字', async () => {
    useUiStore.setState({
      stages: [
        { run_id: 'r1', stage: '规格', seq: 1, state: 'done' },
        { run_id: 'r2', stage: '实现', seq: 2, state: 'active' },
      ],
      team: [{ id: 'a1', role: '后端开发', model_slot: null, status: 'active', avatar_hash: null }],
      streams: { a1: { 1: '写' } },
      thinkings: {},
      streamDone: {},
      timeline: [{
        event: {
          id: 1, project_id: 'p1', kind: 'turn_started', agent_id: 'a1',
          stage_run_id: null, payload: {}, created_at: '',
        },
        message: null,
      }, {
        event: {
          id: 2, project_id: 'p1', kind: 'agent_message', agent_id: 'a1',
          stage_run_id: null, payload: {}, created_at: '',
        },
        message: {
          id: 2, author: 'a1', tokens: [], attachments: [], created_at: '', thinking: '',
          body: '计划：阶段是发布，下一步用 fs_write',
        },
      }, {
        event: {
          id: 3, project_id: 'p1', kind: 'tool_called', agent_id: 'a1',
          stage_run_id: null, payload: { tool: 'fs_read' }, created_at: '',
        },
        message: null,
      }],
    })
    const { el, root } = await render(<StatusLine />)
    const line = el.querySelector('[data-status-line]')!
    expect(line.textContent).toContain('阶段 实现')
    expect(line.textContent).toContain('后端开发')
    expect(line.textContent).toContain('fs_read')
    expect(line.textContent).not.toContain('fs_write')
    expect(line.textContent).not.toContain('发布')
    root.unmount()
  })

  it('什么都没在跑时不画状态行', async () => {
    const { el, root } = await render(<StatusLine />)
    expect(el.querySelector('[data-status-line]')).toBeNull()
    root.unmount()
  })
})
