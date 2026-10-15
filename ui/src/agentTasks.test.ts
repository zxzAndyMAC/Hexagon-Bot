// Agent 详情 Task Rows 分组（ui-redesign 票 09，components D16）。
import { expect, it } from 'vitest'
import { groupTasks, type TaskStep } from './agentTasks'

const s = (id: number, kind: string, extra: Partial<TaskStep> = {}): TaskStep => ({ id, kind, label: kind, time: `t${id}`, ...extra })

it('按回合分组：标题取最后一条消息首行，统计调用与交付', () => {
  const g = groupTasks([
    s(1, 'turn_started'),
    s(2, 'tool_called', { label: 'Read', summary: 'a.ts', ok: true }),
    s(3, 'artifact_delivered', { summary: 'a.ts' }),
    s(4, 'agent_message', { message: '## 列表页与本地存储\n细节' }),
    s(5, 'turn_finished'),
  ], false)
  expect(g).toHaveLength(1)
  expect(g[0]).toMatchObject({ id: 1, title: '列表页与本地存储', state: 'done', calls: 1, delivered: true, time: 't1' })
  expect(g[0].steps.map((x) => x.id)).toEqual([2, 3, 4])
})

it('回合失败标失败；没有消息时标题用最后一步', () => {
  const g = groupTasks([s(1, 'turn_started'), s(2, 'tool_called', { label: 'Run', summary: 'npm run lint', ok: false }), s(3, 'turn_failed')], false)
  expect(g[0]).toMatchObject({ state: 'failed', title: 'Run · npm run lint' })
})

it('只有最后一个未收尾回合且正忙时才算进行中', () => {
  const steps = [s(1, 'turn_started'), s(2, 'tool_called'), s(3, 'turn_started'), s(4, 'tool_called')]
  expect(groupTasks(steps, true).map((g) => g.state)).toEqual(['done', 'running'])
  expect(groupTasks(steps, false).map((g) => g.state)).toEqual(['done', 'done'])
})

it('回合外的零散事件各成一行', () => {
  const g = groupTasks([s(1, 'agent_slept', { label: '休眠' }), s(2, 'tool_called', { label: 'Run', ok: false }), s(3, 'tool_called', { label: 'Read', ok: 'unknown' })], false)
  expect(g.map((x) => [x.title, x.state, x.turn, x.step?.id])).toEqual([['休眠', 'done', false, 1], ['Run', 'failed', false, 2], ['Read', 'unknown', false, 3]])
})
