// 待决坞（ui-redesign 票 04）。由 PendingDialog.test 迁移改写——ADR 0089：遮罩弹窗（role=dialog + 背板）换成
// 输入条上方的待决坞（role=region）。改写处：①「弹出 / 关闭」改为「展开 / 收起成胶囊」，收起后仍渲染胶囊，不是消失；
// ② 坞一次只显示一张卡（分页），旧「弹窗里同时列出所有卡」的断言改为翻页；③ 按钮上键位写成 kbd（顶卡才有），
// 按钮 textContent 不再是纯文案；④ 失速卡按钮顺序照 decisions#cards 改为「知道了 · 再试一次」（次在左、主在右）；
// ⑤ 进出场 motion 撤掉（坞内单卡翻页，不再有列表补位动画）。保留的断言：关闭不销卡、新卡自动展开、
// 原生授权弹窗之上关闭键不作用于坞、最终验收退回、失速两钮语义、未知动作理由必填。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import './i18n'
import i18n from './i18n'
import App from './App'
import { Sidebar } from './components/shell/Sidebar'
import { PendingCard } from './components/PendingCards'
import { PendingDock } from './components/PendingDock'
import { TooltipProvider } from './components/ui/tooltip'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { bindingFor, formatBinding, isMac } from './keymap'
import { usePendingKeys } from './decisions'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const card = (over: Partial<PendingQuestion> = {}): PendingQuestion => ({
  id: 'q1',
  kind: 'permission',
  agent_id: 'a0',
  payload: { tool: 'bash', input: { command: 'cargo test' }, reason: '回归', safety_net: false },
  state: 'queued',
  ...over,
})

const press = (key: string, mod = true) =>
  new KeyboardEvent('keydown', { key, metaKey: mod && isMac, ctrlKey: mod && !isMac, bubbles: true, cancelable: true })

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(<TooltipProvider>{node}</TooltipProvider>) })
  return { el, root }
}

/** App 的全局裁决键接缝：usePendingKeys → handlePendingKey（键只打在坞顶卡上）。 */
function KeyHost() {
  const { onKey } = usePendingKeys()
  window.onkeydown = (e) => { void onKey(e) }
  return null
}

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })

describe('待决坞（ui-redesign 票 04，ADR 0089）', () => {
  const snap = useUiStore.getState()
  let root: Root | null = null

  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    vi.spyOn(api, 'proposals').mockResolvedValue([])
    vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValue(null)
    useUiStore.setState({
      pending: [], reviewRows: [], pendingDialogOpen: false, dismissedPendingKeys: [], hiddenPending: [], pendingError: null,
      toasts: [], modalScope: 'workbench', confirmReq: null, team: [{ id: 'a0', role: 'QA', status: 'active', model_slot: 'chat', avatar_hash: null, model: null, duty: null, superior: null }],
    })
  })

  afterEach(async () => {
    if (root) {
      await act(async () => { root!.unmount() })
      root = null
    }
    window.onkeydown = null
    document.body.innerHTML = ''
    vi.restoreAllMocks()
    useUiStore.setState(snap, true)
    await i18n.changeLanguage('en')
  })

  it('App：坞停在输入条正上方，不是遮罩弹窗；侧栏徽标计数', async () => {
    const view = await render(<App />)
    root = view.root
    await act(async () => { await new Promise((r) => setTimeout(r, 30)) })
    expect(view.el.querySelector('[role="dialog"][aria-modal="true"]')).toBeNull()
    const dock = view.el.querySelector('[data-dock="open"]')
    expect(dock).not.toBeNull()
    // 坞与输入条同在对话主区，且在输入条之前（Tab 序：对话 → 坞 → 输入条）
    const composer = view.el.querySelector('#composer-input')!
    expect(dock!.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(view.el.querySelector('[data-pending-count]')?.getAttribute('data-pending-count')).toBe(
      String(useUiStore.getState().pending.length + useUiStore.getState().reviewRows.length),
    )
  })

  it('按 SEVERITY 分页「1 / N」，键位只写在顶卡；翻到别页时 ⌘↵ 仍只裁决顶卡', async () => {
    const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue()
    const confirmWt = vi.spyOn(api, 'confirmWorktree').mockResolvedValue(undefined as never)
    vi.spyOn(api, 'pendingQuestions').mockImplementation(async () => [...useUiStore.getState().pending])
    useUiStore.setState({
      pending: [
        card(),
        card({ id: 'wt', kind: 'worktree', payload: { operation: 'create', path: '/tmp/wt', baseline: 'main', baseline_commit: 'abcdef1234', session_id: 's1' } }),
        card({ id: 'q3', kind: 'stamp', payload: { stage: '实现', run_id: 'r1' } }),
      ],
    })
    const { el, root: r } = await render(<><KeyHost /><PendingDock /></>)
    root = r
    await flush()
    const dock = el.querySelector('[data-dock="open"]')!
    expect(dock.getAttribute('data-pending-top')).toBe('wt')
    expect(dock.textContent).toContain('1 / 3')
    // 顶卡 = 工作树确认（与发布同档）：D8 呈现——红 err 图标块、kv、创建用 primary、按钮带键位
    expect(dock.querySelector('.dock-ic.err')).not.toBeNull()
    expect(dock.textContent).toContain('/tmp/wt')
    expect(dock.textContent).toContain('abcdef12')
    const create = dock.querySelector<HTMLButtonElement>('button[data-variant="primary"]')!
    expect(create.textContent).toContain(formatBinding(bindingFor('approve')))
    expect(dock.textContent).not.toMatch(/记住/)

    // 翻到第 2 张：阶段盖章，不是顶卡 → 按钮不带键位
    const next = [...dock.querySelectorAll('button')].find((b) => b.getAttribute('aria-label') === '下一张')!
    await act(async () => next.click())
    expect(dock.textContent).toContain('2 / 3')
    expect(dock.textContent).toContain('「实现」阶段等你盖章')
    expect(dock.textContent).not.toContain(formatBinding(bindingFor('approve')))

    // ⌘↵ 仍作用于顶卡（工作树），不是眼前这张
    await act(async () => { window.dispatchEvent(press('Enter')) })
    await flush()
    expect(confirmWt).toHaveBeenCalledWith('wt')
    expect(answer).not.toHaveBeenCalled()
  })

  it('⌘Esc 收起成胶囊：徽标数不变、不销卡；新卡到达自动展开；点胶囊再展开', async () => {
    const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue()
    useUiStore.setState({ pending: [card(), card({ id: 'pub', kind: 'publish', payload: { remote: 'origin', baseline: 'main' } })] })
    const { el, root: r } = await render(
      <><Sidebar onSearch={() => {}} onSettings={() => {}} onProjectClosed={() => {}} /><PendingDock /></>,
    )
    root = r
    await flush()
    expect(el.querySelector('[data-dock="open"]')).not.toBeNull()
    const collapse = el.querySelector<HTMLButtonElement>('[aria-label="收起待决坞"]')!
    expect(collapse).not.toBeNull()
    await act(async () => { window.dispatchEvent(press('Escape')) })
    const pill = el.querySelector<HTMLButtonElement>('[data-dock="collapsed"]')!
    expect(pill).not.toBeNull()
    expect(pill.textContent).toContain('2 件待决 · 最严重：远程发布')
    expect(el.querySelector('[data-pending-count]')?.getAttribute('data-pending-count')).toBe('2')
    expect(answer).not.toHaveBeenCalled()
    expect(useUiStore.getState().pending).toHaveLength(2)

    await act(async () => { useUiStore.setState({ pending: [...useUiStore.getState().pending, card({ id: 'q-new' })] }) })
    expect(el.querySelector('[data-dock="open"]')).not.toBeNull()

    await act(async () => { useUiStore.getState().closePendingDialog() })
    await act(async () => el.querySelector<HTMLButtonElement>('[data-dock="collapsed"]')!.click())
    expect(el.querySelector('[data-dock="open"]')).not.toBeNull()
  })

  it('modalScope 隔离：设置页 / 命令面板时 ⌘↵ 不裁决并提示，⌘Esc 不收坞；按钮仍可点', async () => {
    const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue()
    useUiStore.setState({ pending: [card()], modalScope: 'settings' })
    const { el, root: r } = await render(<><KeyHost /><PendingDock /></>)
    root = r
    await flush()
    await act(async () => { window.dispatchEvent(press('Enter')) })
    expect(answer).not.toHaveBeenCalled()
    expect(useUiStore.getState().toasts.at(-1)?.text).toBe(i18n.t('decisions.scopeBlocked'))
    await act(async () => { window.dispatchEvent(press('Escape')) })
    expect(el.querySelector('[data-dock="open"]')).not.toBeNull()
    useUiStore.setState({ modalScope: 'palette' })
    await act(async () => { window.dispatchEvent(press('Enter')) })
    expect(answer).not.toHaveBeenCalled()
  })

  it('保存即消卡：点批准卡立即离坞；失败时卡回坞顶并在底栏上方写红字原因', async () => {
    let finish!: (v?: unknown) => void
    const answer = vi.spyOn(api, 'answerPermission').mockImplementation(() => new Promise((res, rej) => { finish = (v) => (v instanceof Error ? rej(v) : res(undefined)) }))
    useUiStore.setState({ pending: [card(), card({ id: 'q2', payload: { tool: 'bash', input: { command: 'ls' } } })], invalidate: async () => {} })
    const { el, root: r } = await render(<PendingDock />)
    root = r
    await flush()
    const allow = [...el.querySelectorAll('button')].find((b) => b.textContent?.startsWith('允许一次'))!
    await act(async () => allow.click())
    expect(answer).toHaveBeenCalledWith('q1', true)
    // 提交中：q1 已离坞，坞显示下一张
    expect(el.textContent).not.toContain('$ cargo test')
    expect(el.textContent).toContain('$ ls')
    await act(async () => { finish(new Error('boom')) })
    await flush()
    expect(el.querySelector('[data-pending-top]')?.getAttribute('data-pending-top')).toBe('q1')
    expect(el.textContent).toContain('$ cargo test')
    expect(el.querySelector('[data-decision-error]')?.textContent).toContain('boom')
  })

  it('托管事项不计入、不进坞、不吃快捷键（宿主的 owner_pending 已排除托管卡）', async () => {
    const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue()
    useUiStore.setState({ pending: [] })
    const { el, root: r } = await render(
      <><KeyHost /><Sidebar onSearch={() => {}} onSettings={() => {}} onProjectClosed={() => {}} /><PendingDock /></>,
    )
    root = r
    await flush()
    expect(el.querySelector('[data-dock]')).toBeNull()
    expect(el.querySelector('[data-pending-count]')).toBeNull()
    await act(async () => { window.dispatchEvent(press('Enter')) })
    expect(answer).not.toHaveBeenCalled()
  })

  it('原生授权弹窗打开时 ⌘Esc 不作用于背后的坞', async () => {
    useUiStore.setState({ pending: [card()] })
    const { root: r } = await render(<><PendingDock /><dialog open>System permissions</dialog></>)
    root = r
    await flush()
    expect(useUiStore.getState().pendingDialogOpen).toBe(true)
    await act(async () => { window.dispatchEvent(press('Escape')) })
    expect(useUiStore.getState().pendingDialogOpen).toBe(true)
  })

  it('最终验收退回把阶段和修改意见交给 reject_stamp', async () => {
    const reject = vi.spyOn(api, 'rejectStamp').mockResolvedValue({ action: 'stamp_rejected', reopened_seq: 0, run_id: 'sr' })
    useUiStore.setState({
      invalidate: async () => {},
      stages: [
        { run_id: 'r0', stage: '需求', seq: 0, state: 'done' },
        { run_id: 'r1', stage: '合入', seq: 1, state: 'waiting_stamp' },
      ],
    })
    const { el, root: r } = await render(
      <PendingCard top={false} q={card({ kind: 'stamp', payload: { stage: '合入', run_id: 'r1', final_acceptance: true } })} />,
    )
    root = r
    const stage = el.querySelector('select') as HTMLSelectElement
    const note = el.querySelector('input') as HTMLInputElement
    stage.value = '需求'
    note.value = '把验收写具体'
    const btn = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('驳回'))
    await act(async () => { btn!.click() })
    expect(reject).toHaveBeenCalledWith('需求', '把验收写具体')
  })

  it('stall-watch 票 02：失速卡两钮（次左主右，ADR 0089 换皮），键位在顶卡按钮上，各走各的命令', async () => {
    const retry = vi.spyOn(api, 'stallRetry').mockResolvedValue()
    const ack = vi.spyOn(api, 'stallAck').mockResolvedValue()
    useUiStore.setState({ invalidate: async () => {} })
    const { el, root: r } = await render(
      <PendingCard top q={card({ id: 'qs', kind: 'stall', payload: { branch: 'no_reply', retry: true, role: '后端' } })} />,
    )
    root = r
    const btns = [...el.querySelectorAll('button')]
    expect(btns.map((b) => b.querySelector('kbd') ? b.textContent!.replace(b.querySelector('kbd')!.textContent!, '') : b.textContent)).toEqual(['知道了', '再试一次'])
    expect(btns[0].title).toBe(formatBinding(bindingFor('reject')))
    expect(btns[1].title).toBe(formatBinding(bindingFor('approve')))
    expect(el.textContent).toContain('重触发一次后仍没有可见回复')
    expect(el.textContent).not.toMatch(/改派/)
    await act(async () => { btns[1].click() })
    expect(retry).toHaveBeenCalledWith('qs')
    await act(async () => { btns[0].click() })
    expect(ack).toHaveBeenCalledWith('qs')
  })

  it('stall-watch 票 02/03：只剩「知道了」的失速卡', async () => {
    const { el, root: r } = await render(
      <PendingCard top={false} q={card({ id: 'qs3', kind: 'stall', agent_id: null, payload: { branch: 'idle_spin', retry: false } })} />,
    )
    root = r
    expect([...el.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['知道了'])
    expect(el.textContent).toContain('有回复，但进度没有变化')
  })

  it('未知动作（R-03）：理由空时放弃追查与重试禁用；不勾确认时放弃追查禁用；核对是 default', async () => {
    const { el, root: r } = await render(<PendingCard top q={card({ kind: 'recovery', payload: {
      sub: 'tool_outcome_unknown', action_id: 'a1', tool: 'fs_write', target: 'db/migrate.sql', host_project_root: '/p',
    } })} />)
    root = r
    const btn = (a: string) => el.querySelector<HTMLButtonElement>(`button[data-action="${a}"]`)!
    expect(btn('reconcileAction').dataset.variant).toBe('default')
    expect(btn('abandonAction').dataset.variant).toBe('quiet')
    expect(btn('retryAction').dataset.variant).toBe('default')
    expect(btn('abandonAction').disabled).toBe(true)
    expect(btn('retryAction').disabled).toBe(true)
    const input = el.querySelector('input:not([type])') as HTMLInputElement
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, '文件已存在且内容正确')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(btn('abandonAction').disabled).toBe(true)
    expect(el.querySelector('[data-blocked]')?.textContent).toContain('我了解放弃追查后不会重放该动作')
    await act(async () => el.querySelector<HTMLButtonElement>('[data-ack="abandon"]')!.click())
    expect(btn('abandonAction').disabled).toBe(false)
    expect(btn('retryAction').disabled).toBe(true)
    await act(async () => el.querySelector<HTMLButtonElement>('[data-ack="duplicate"]')!.click())
    expect(btn('retryAction').disabled).toBe(false)
  })
})

it('动作恢复卡区分未知与已授权未执行（可靠性 08）', async () => {
  await i18n.changeLanguage('zh-CN')
  const saved = useUiStore.getState()
  useUiStore.setState({ invalidate: async () => {} })
  const resume = vi.spyOn(api, 'resumeToolAction').mockResolvedValue()
  const unknown = await render(<PendingCard q={card({ kind: 'recovery', payload: { sub: 'tool_outcome_unknown', action_id: 'action1' } })} top />)
  expect(unknown.el.textContent).toContain('工具结果未知')
  expect(unknown.el.textContent).toContain('普通批准不会重试')
  // Reliability 11 + R-03（票 04 定名：核对 / 放弃追查 / 重试）：普通批准仍不能静默重试未知动作。
  const actions = [...unknown.el.querySelectorAll<HTMLButtonElement>('button[data-action]')]
  expect(actions.map((b) => b.textContent?.replace(/(Ctrl|Alt|⌘|⌥).*$/, ''))).toEqual(['核对', '放弃追查', '重试'])
  expect(actions[1].disabled).toBe(true)
  expect(actions[2].disabled).toBe(true)
  expect(actions[0].title).toContain(formatBinding(bindingFor('reconcileAction')))
  await act(async () => unknown.root.unmount())
  const ready = await render(<PendingCard q={card({ kind: 'recovery', payload: { sub: 'tool_action_ready', action_id: 'action2' } })} top />)
  const button = ready.el.querySelector<HTMLButtonElement>('button[data-variant="primary"]')!
  expect(button.textContent).toContain(formatBinding(bindingFor('approve')))
  await act(async () => button.click())
  expect(resume).toHaveBeenCalledWith('action2')
  await act(async () => ready.root.unmount())
  vi.restoreAllMocks()
  useUiStore.setState(saved, true)
  await i18n.changeLanguage('en')
})

it('写入待决显示绑定目标和变更冲突说明', async () => {
  await i18n.changeLanguage('zh-CN')
  vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValue(null)
  const { el, root } = await render(<PendingCard top q={card({ payload: {
    tool: 'fs_write', input: { path: 'notes.md' }, action_id: 'action1', write_targets: ['notes.md'],
  } })} />)
  expect(el.textContent).toContain('写入前会重新核对目标')
  expect(el.textContent).toContain('notes.md')
  await act(async () => root.unmount())
  el.remove()
  vi.restoreAllMocks()
  await i18n.changeLanguage('en')
})
