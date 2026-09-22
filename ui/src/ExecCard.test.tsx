// exec-cards 票 02：ExecCard 壳 + 分体行为面——
// 收展、徽标有无（D6 缺字段不挂）、复制翻转、bash 在途流渲染。
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { ToolExecCard } from './components/ExecCard'
import { useUiStore } from './store'
import type { TimelineItem } from './api'
import type { EventKind } from './gen/EventKind'
import type { ToolCall } from './agentSteps'

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

const callOf = (tool: string, input: Record<string, unknown>, output?: Record<string, unknown>, ok = true): ToolCall => ({
  called: ev(1, 'tool_called', { tool, input, seq: 'r1:i0' }),
  result: output === undefined && !ok
    ? ev(2, 'tool_result', { tool, ok, result: { error: 'boom' } })
    : output !== undefined
      ? ev(2, 'tool_result', { tool, ok, result: { output } })
      : undefined,
})

const open = async (el: HTMLElement) => {
  await act(async () => {
    el.querySelector<HTMLElement>('.exec-toggle')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  })
}

beforeEach(() => {
  document.body.innerHTML = ''
  useUiStore.setState({
    team: [{ id: 'a1', role: '后端开发', model_slot: null, status: 'active', avatar_hash: null }],
    toolStreams: {},
    tabs: [{ id: 'timeline', kind: 'timeline', title: 'timeline' }] as never,
    activeTab: 'timeline',
  })
})

describe('ExecCard（exec-cards 票 02）', () => {
  it('fs_patch：result 带 diff 统计时挂 +N −M 徽标，展开渲 old/new 差异行', async () => {
    const call = callOf('fs_patch',
      { path: 'src/a.ts', old: 'let x = 1', new: 'let x = 2\nlet y = 3' },
      { patched: 'src/a.ts', diff_added: 2, diff_removed: 1 })
    const { el, root } = await render(<ToolExecCard call={call} />)
    expect(el.textContent).toContain('src/a.ts')
    expect(el.querySelector('.exec-stat.ins')!.textContent).toBe('+2')
    expect(el.querySelector('.exec-stat.del')!.textContent).toBe('−1')
    expect(el.querySelector('.exec-body')).toBeNull() // 收态无体
    await open(el)
    expect(el.querySelector('.exec-body')).not.toBeNull()
    // DiffView 行：-旧 +新
    const body = el.querySelector('.exec-body')!.textContent!
    expect(body).toContain('let x = 1')
    expect(body).toContain('let x = 2')
    root.unmount()
  })

  it('fs_patch：无 diff 字段不挂徽标（D6 缺数据 ≠ 零变化）', async () => {
    const call = callOf('fs_patch', { path: 'src/a.ts', old: 'a', new: 'b' }, { patched: 'src/a.ts' })
    const { el, root } = await render(<ToolExecCard call={call} />)
    expect(el.querySelector('.exec-stat')).toBeNull()
    root.unmount()
  })

  it('bash：在途渲 toolStreams 实时缓冲，落定定格 result.output', async () => {
    const live = callOf('bash', { cmd: 'cargo test' }) // 无 result=在途
    useUiStore.setState({ toolStreams: { 'a1:r1:i0': 'compiling…\ntest 1 ok\n' } })
    const { el, root } = await render(<ToolExecCard call={live} />)
    expect(el.querySelector('.spinner-ring, [class*=spinner], svg')).not.toBeNull()
    await open(el)
    expect(el.querySelector('.exec-out')!.textContent).toContain('test 1 ok')
    root.unmount()

    const done = callOf('bash', { cmd: 'cargo test' }, { stdout: 'all 5 passed', stderr: '', exit_code: 0 })
    useUiStore.setState({ toolStreams: {} })
    const again = await render(<ToolExecCard call={done} />)
    await open(again.el)
    expect(again.el.querySelector('.exec-out')!.textContent).toContain('all 5 passed')
    again.root.unmount()
  })

  it('bash：非零 exit_code 挂 exit 徽标', async () => {
    const call = callOf('bash', { cmd: 'false' }, { stdout: '', stderr: '', exit_code: 1 })
    const { el, root } = await render(<ToolExecCard call={call} />)
    expect(el.textContent).toContain('exit 1')
    root.unmount()
  })

  it('fs_write：bytes 徽标 + JSON 明细，无 diff 徽标（D4 不装伪统计）', async () => {
    const call = callOf('fs_write', { path: 'specs/prd.md', bytes: 2048 }, { written: 'specs/prd.md' })
    const { el, root } = await render(<ToolExecCard call={call} />)
    expect(el.textContent).toContain('2.0 KB')
    expect(el.querySelector('.exec-stat')).toBeNull()
    root.unmount()
  })

  it('复制钮写剪贴板并翻转已复制态', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    const call = callOf('bash', { cmd: 'cargo build' }, { stdout: 'ok', stderr: '', exit_code: 0 })
    const { el, root } = await render(<ToolExecCard call={call} />)
    const btn = el.querySelector('.exec-copy')!
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(writeText).toHaveBeenCalledWith('cargo build')
    expect(btn.querySelector('svg')).not.toBeNull()
    root.unmount()
  })
})
