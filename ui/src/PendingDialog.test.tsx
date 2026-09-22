import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import './i18n'
import i18n from './i18n'
import App from './App'
import { TopBar } from './components/TopBar'
import { PendingDialog } from './components/PendingCards'
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
})
