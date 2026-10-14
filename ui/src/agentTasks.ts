// Agent 详情「活动」页的 Task Rows 分组（components D16，canvas#agent；ui-redesign 票 09）。
// 一行 = 一个回合（turn_started 起，turn_finished / turn_failed 收）；回合外的零散事件各成一行。
// 状态：失败（turn_failed / 收尾前有失败的工具调用不算，只看回合收尾）· 完成 · 进行中（只有最后一个未收尾回合
// 且该 Agent 正忙时才算进行中——更早的未收尾回合是窗口截断或旧数据，按完成显示，不画假的转圈）。
// 标题：回合里最后一条 Agent 消息的首行；没有消息时用最后一步的「动作 · 对象」。

export type TaskStep = {
  id: number
  kind: string
  label: string
  summary?: string
  message?: string
  divider?: boolean
  time: string
  /** 工具调用结果：true 成功 / false 失败 / undefined 在途或不适用。 */
  ok?: boolean | 'unknown' | null
}

export type TaskState = 'running' | 'done' | 'failed'

export type TaskGroup<S extends TaskStep = TaskStep> = {
  id: number
  title: string
  state: TaskState
  /** 回合内的子步骤（不含 turn_started / 收尾事件本身）。 */
  steps: S[]
  calls: number
  delivered: boolean
  time: string
  /** 回合开始事件的阶段名（turn_started 的 label 已带）。 */
  turn: boolean
}

const firstLine = (s: string) => s.split('\n').map((l) => l.replace(/^[#>*\-\s]+/, '').trim()).find(Boolean) ?? ''
const clip = (s: string, n = 80) => (s.length > n ? `${s.slice(0, n - 1)}…` : s)

function titleOf(steps: TaskStep[], fallback: string): string {
  for (let i = steps.length - 1; i >= 0; i--) if (steps[i].message) return clip(firstLine(steps[i].message!))
  const last = steps[steps.length - 1]
  if (last) return clip(last.summary ? `${last.label} · ${last.summary}` : last.label)
  return fallback
}

export function groupTasks<S extends TaskStep>(steps: S[], busy: boolean): TaskGroup<S>[] {
  const out: TaskGroup<S>[] = []
  let open: { start: S; steps: S[] } | null = null
  const close = (start: S, inner: S[], end: S | null) => {
    out.push({
      id: start.id,
      title: titleOf(inner, start.label),
      state: end?.kind === 'turn_failed' ? 'failed' : end ? 'done' : 'running',
      steps: inner,
      calls: inner.filter((s) => s.kind === 'tool_called').length,
      delivered: inner.some((s) => s.kind === 'artifact_delivered'),
      time: start.time,
      turn: true,
    })
  }
  for (const s of steps) {
    if (s.kind === 'turn_started') {
      if (open) close(open.start, open.steps, null)
      open = { start: s, steps: [] }
      continue
    }
    if (open && (s.kind === 'turn_finished' || s.kind === 'turn_failed')) {
      close(open.start, open.steps, s)
      open = null
      continue
    }
    if (open) { open.steps.push(s); continue }
    out.push({
      id: s.id,
      title: clip(s.message ? firstLine(s.message) : s.summary ? `${s.label} · ${s.summary}` : s.label),
      state: s.kind === 'turn_failed' || s.ok === false ? 'failed' : 'done',
      steps: [],
      calls: s.kind === 'tool_called' ? 1 : 0,
      delivered: s.kind === 'artifact_delivered',
      time: s.time,
      turn: false,
    })
  }
  if (open) close(open.start, open.steps, null)
  // 只有最后一个未收尾回合、且 Agent 正忙，才是进行中
  return out.map((g, i) => (g.state === 'running' && !(busy && i === out.length - 1) ? { ...g, state: 'done' } : g))
}
