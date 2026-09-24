import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import './i18n'
import i18n from './i18n'
import App from './App'
import { TopBar } from './components/TopBar'
import { PendingCard, PendingCards, PendingDialog } from './components/PendingCards'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import { bindingFor, formatBinding } from './keymap'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const card = (over: Partial<PendingQuestion> = {}): PendingQuestion => ({
  id: 'q1',
  kind: 'permission',
  agent_id: 'a0',
  payload: { tool: 'bash', input: { command: 'cargo test' }, reason: '回归', safety_net: false },
  state: 'queued',
  ...over,
})

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

describe('待决弹窗（hands-free 票 05）', () => {
  const snap = useUiStore.getState()
  let root: Root | null = null

  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
    vi.spyOn(api, 'proposals').mockResolvedValue([])
    useUiStore.setState({
      pending: [],
      reviewRows: [],
      pendingDialogOpen: false,
      dismissedPendingKeys: [],
      toasts: [],
      modalScope: 'workbench',
      confirmReq: null,
    })
  })

  afterEach(async () => {
    if (root) {
      await act(async () => { root!.unmount() })
      root = null
    }
    document.body.innerHTML = ''
    vi.restoreAllMocks()
    useUiStore.setState(snap, true)
    await i18n.changeLanguage('en')
  })

  it('有待决时弹出，工作台不再有常驻待决区', async () => {
    const view = await render(<App />)
    root = view.root
    await act(async () => { await new Promise((r) => setTimeout(r, 30)) })
    expect(view.el.querySelector('#pending-zone')).toBeNull()
    expect(view.el.querySelector('.zone-splitter')).toBeNull()
    expect(view.el.querySelector('[role="dialog"]')).not.toBeNull()
    expect(view.el.querySelector('[data-pending-count]')?.getAttribute('data-pending-count')).toBe(
      String(useUiStore.getState().pending.length),
    )
  })

  it('弹窗里能批准和拒绝，关掉不销卡，徽标可再打开', async () => {
    const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue()
    // 批准会走 invalidate→pending_questions。钉住原列表，避免 mock 的 7 张卡盖掉「关掉不销卡」。
    vi.spyOn(api, 'pendingQuestions').mockImplementation(async () => [...useUiStore.getState().pending])
    useUiStore.setState({
      pending: [card(), card({ id: 'q2', payload: { tool: 'bash', input: { command: 'ls' }, reason: '看目录' } })],
    })
    const { el, root: r } = await render(
      <>
        <TopBar onSettings={() => {}} onProjectClosed={() => {}} />
        <PendingDialog />
      </>,
    )
    root = r
    await act(async () => { await Promise.resolve() })

    const dialog = el.querySelector('[role="dialog"]')
    expect(dialog).not.toBeNull()
    expect(dialog?.getAttribute('aria-label')).toBe(i18n.t('cards.pendingDialog'))

    const badge = el.querySelector('[data-pending-count]') as HTMLButtonElement
    expect(badge.getAttribute('data-pending-count')).toBe('2')
    expect(badge.textContent).toContain('2')
    expect(badge.title).toContain(formatBinding(bindingFor('approve')))
    expect(badge.title).toContain(formatBinding(bindingFor('reject')))

    const deny = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('拒绝'))
    expect(deny).toBeTruthy()
    await act(async () => { deny!.click() })
    expect(answer).toHaveBeenCalledWith('q1', false)

    // 关掉是收起，不是驳回：张数还在，卡还在。
    answer.mockClear()
    const close = el.querySelector('[aria-label="关闭"]') as HTMLButtonElement
    expect(close.title).toContain(formatBinding(bindingFor('dismissPending')))
    await act(async () => { close.click() })
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    expect(answer).not.toHaveBeenCalled()
    expect(useUiStore.getState().pending).toHaveLength(2)
    expect(el.querySelector('[data-pending-count]')?.getAttribute('data-pending-count')).toBe('2')

    await act(async () => { badge.click() })
    expect(el.querySelector('[role="dialog"]')).not.toBeNull()
  })

  it('没有待决时不弹出，数量为空', async () => {
    const { el, root: r } = await render(
      <>
        <TopBar onSettings={() => {}} onProjectClosed={() => {}} />
        <PendingDialog />
      </>,
    )
    root = r
    await act(async () => { await Promise.resolve() })
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    expect(el.querySelector('[data-pending-count]')).toBeNull()
  })

  it('关掉之后新卡再次弹出，离开再回来徽标仍在', async () => {
    useUiStore.setState({ pending: [card()] })
    const { el, root: r } = await render(
      <>
        <TopBar onSettings={() => {}} onProjectClosed={() => {}} />
        <PendingDialog />
      </>,
    )
    root = r
    await act(async () => { await Promise.resolve() })
    // 关闭键走键位表（非 mac 的 mod = Ctrl），不是把卡驳回。
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', ctrlKey: true, bubbles: true }))
    })
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    expect(el.querySelector('[data-pending-count]')?.getAttribute('data-pending-count')).toBe('1')

    await act(async () => {
      useUiStore.setState({ pending: [card(), card({ id: 'q-new' })] })
    })
    expect(el.querySelector('[role="dialog"]')).not.toBeNull()
    expect(el.querySelector('[data-pending-count]')?.getAttribute('data-pending-count')).toBe('2')
  })

  // beautiful-ui 票 01：进出场动画 smoke——增卡渲染、移除后 DOM 收走，不炸。
  it('卡片进出场：新增渲染、裁决移除后收走', async () => {
    const q1 = card({ id: 'q1', payload: { tool: 'bash', input: { command: 'cargo test' }, reason: '回归' } })
    const q2 = card({ id: 'q2', payload: { tool: 'bash', input: { command: 'npm run lint' }, reason: '检查' } })
    useUiStore.setState({ pending: [q1] })
    const { el, root: r } = await render(<PendingCards />)
    root = r
    expect(el.textContent).toContain('cargo test')

    await act(async () => { useUiStore.setState({ pending: [q1, q2] }) })
    expect(el.textContent).toContain('npm run lint')

    await act(async () => { useUiStore.setState({ pending: [q2] }) })
    await act(async () => { await new Promise((r2) => setTimeout(r2, 500)) })
    expect(el.textContent).not.toContain('cargo test')
    expect(el.textContent).toContain('npm run lint')
  })

  it('最终验收退回把阶段和修改意见交给 reject_stamp', async () => {
    const reject = vi.spyOn(api, 'rejectStamp').mockResolvedValue({ action: 'stamp_rejected', reopened_seq: 0, run_id: 'sr' })
    useUiStore.setState({
      stages: [
        { run_id: 'r0', stage: '需求', seq: 0, state: 'done' },
        { run_id: 'r1', stage: '合入', seq: 1, state: 'waiting_stamp' },
      ],
    })
    const { el, root: r } = await render(
      <PendingCard
        top={false}
        q={card({
          kind: 'stamp',
          payload: { stage: '合入', run_id: 'r1', final_acceptance: true },
        })}
      />,
    )
    root = r
    const stage = el.querySelector('select') as HTMLSelectElement
    const note = el.querySelector('input') as HTMLInputElement
    expect(stage).not.toBeNull()
    expect(note).not.toBeNull()
    stage.value = '需求'
    note.value = '把验收写具体'
    const btn = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('驳回'))
    expect(btn).toBeTruthy()
    await act(async () => { btn!.click() })
    expect(reject).toHaveBeenCalledWith('需求', '把验收写具体')
  })

  it('stall-watch 票 02：失速卡两钮，tooltip 显示当前绑定，按钮各走各的命令', async () => {
    const retry = vi.spyOn(api, 'stallRetry').mockResolvedValue()
    const ack = vi.spyOn(api, 'stallAck').mockResolvedValue()
    const { el, root: r } = await render(
      <PendingCard
        top
        q={card({ id: 'qs', kind: 'stall', payload: { branch: 'no_reply', retry: true, role: '后端' } })}
      />,
    )
    root = r
    const btns = [...el.querySelectorAll('button')]
    expect(btns.map((b) => b.textContent?.split(' ')[0])).toEqual(['再试一次', '知道了'])
    expect(btns[0].title).toBe(formatBinding(bindingFor('approve')))
    expect(btns[1].title).toBe(formatBinding(bindingFor('reject')))
    expect(el.textContent).toContain('重触发一次后仍没有可见回复')
    expect(el.textContent).not.toMatch(/改派/)
    await act(async () => { btns[0].click() })
    expect(retry).toHaveBeenCalledWith('qs')
    await act(async () => { btns[1].click() })
    expect(ack).toHaveBeenCalledWith('qs')
  })

  it('stall-watch 票 02：同一段失速的第二张卡只留「知道了」', async () => {
    const { el, root: r } = await render(
      <PendingCard
        top
        q={card({ id: 'qs2', kind: 'stall', payload: { branch: 'no_reply', retry: false, role: '后端' } })}
      />,
    )
    root = r
    const btns = [...el.querySelectorAll('button')]
    expect(btns).toHaveLength(1)
    expect(btns[0].textContent).toContain('知道了')
    expect(el.textContent).toContain('只剩「知道了」')
  })

  it('stall-watch 票 03：无项目经理的空转卡同样没有再试一次', async () => {
    const { el, root: r } = await render(
      <PendingCard top={false} q={card({ id: 'qs3', kind: 'stall', agent_id: null, payload: { branch: 'idle_spin', retry: false } })} />,
    )
    root = r
    expect([...el.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['知道了'])
    expect(el.textContent).toContain('有回复，但进度没有变化')
  })
})
