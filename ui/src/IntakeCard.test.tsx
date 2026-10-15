import { describe, expect, it, vi, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import i18n from './i18n'
import { IntakeCard } from './components/IntakeCard'
import { TooltipProvider } from './components/ui/tooltip'
import { api } from './api'
import { useUiStore } from './store'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(<TooltipProvider>{node}</TooltipProvider>) })
  return { el, root }
}

const button = (el: HTMLElement, text: string) =>
  Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.includes(text)) as HTMLButtonElement

afterEach(() => {
  document.body.innerHTML = ''
  useUiStore.setState({ intakeDraft: false, projectRoot: null })
  vi.restoreAllMocks()
})

// 票 17 的草案条在 ui-redesign 票 05 升格为对话里的开场草案卡（components C13，ADR 0088 对话主区）：
// 原断言「唯一按钮是写入、title 带键位」改为三个动作层级分明；键位改由 Tooltip 显示（ADR 0051）。
describe('开场草案卡（票 17 → ui-redesign 票 05）', () => {
  it('没有草案时不占位', async () => {
    useUiStore.setState({ intakeDraft: false })
    const { el } = await render(<IntakeCard />)
    expect(el.querySelector('[data-testid=intake-card]')).toBeNull()
  })

  it('写入（主）/ 先不写 / 编辑 三个动作，草案卡不是弹窗', async () => {
    await i18n.changeLanguage('zh-CN')
    useUiStore.setState({ intakeDraft: true, projectRoot: '/p/a' })
    vi.spyOn(api, 'intakeDraft').mockResolvedValue('## Purpose\nx\n')
    const confirm = vi.spyOn(api, 'confirmIntakeBrief').mockResolvedValue(undefined)
    const { el } = await render(<IntakeCard />)
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    const write = button(el, '写入 AGENTS.md')
    expect(write.dataset.variant).toBe('primary')
    expect(button(el, '先不写').dataset.variant).toBe('quiet')
    // gap B1：编辑有了 core 命令（update_intake_draft），不再禁用
    expect(button(el, '编辑').dataset.variant).toBe('ghost')
    expect(button(el, '编辑').getAttribute('aria-disabled')).not.toBe('true')
    await act(async () => { write.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(confirm).toHaveBeenCalledOnce()
    expect(useUiStore.getState().intakeDraft).toBe(false)
    await i18n.changeLanguage('en')
  })

  // gap B1（2026-10-15）：卡内 inset 预览读 core 草案正文（原先没有读草案的 IPC，不出预览）
  it('inset 预览显示草案正文', async () => {
    useUiStore.setState({ intakeDraft: true, projectRoot: '/p/c' })
    vi.spyOn(api, 'intakeDraft').mockResolvedValue('## Purpose\nOffline todo PWA\n')
    const { el } = await render(<IntakeCard />)
    expect(el.querySelector('[data-testid=intake-preview]')?.textContent).toContain('Offline todo PWA')
  })

  it('编辑后保存整份写回草案，不落盘；Esc 取消不写', async () => {
    useUiStore.setState({ intakeDraft: true, projectRoot: '/p/d' })
    vi.spyOn(api, 'intakeDraft').mockResolvedValue('## Purpose\nold\n')
    const update = vi.spyOn(api, 'updateIntakeDraft').mockResolvedValue(undefined)
    const confirm = vi.spyOn(api, 'confirmIntakeBrief').mockResolvedValue(undefined)
    const { el } = await render(<IntakeCard />)
    await act(async () => { button(el, 'Edit').dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const box = el.querySelector('[data-testid=intake-edit]') as HTMLTextAreaElement
    expect(box.value).toBe('## Purpose\nold\n')
    await act(async () => { box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
    expect(el.querySelector('[data-testid=intake-edit]')).toBeNull()
    expect(update).not.toHaveBeenCalled()

    await act(async () => { button(el, 'Edit').dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const box2 = el.querySelector('[data-testid=intake-edit]') as HTMLTextAreaElement
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!
    await act(async () => { setter.call(box2, '## Purpose\nnew\n'); box2.dispatchEvent(new Event('input', { bubbles: true })) })
    await act(async () => { button(el, 'Save draft').dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(update).toHaveBeenCalledWith('## Purpose\nnew\n')
    expect(confirm).not.toHaveBeenCalled()
    expect(el.querySelector('[data-testid=intake-preview]')?.textContent).toContain('new')
  })

  // gap B1：「先不写」原先只在本次收起（重开又出现）；现在持久丢弃草案（discard_intake_draft）
  it('先不写持久丢弃草案，不调写入', async () => {
    useUiStore.setState({ intakeDraft: true, projectRoot: '/p/b' })
    vi.spyOn(api, 'intakeDraft').mockResolvedValue('## Purpose\nx\n')
    const discard = vi.spyOn(api, 'discardIntakeDraft').mockResolvedValue(undefined)
    const confirm = vi.spyOn(api, 'confirmIntakeBrief').mockResolvedValue(undefined)
    const { el } = await render(<IntakeCard />)
    await act(async () => { button(el, 'Not now').dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(discard).toHaveBeenCalledOnce()
    expect(el.querySelector('[data-testid=intake-card]')).toBeNull()
    expect(confirm).not.toHaveBeenCalled()
    expect(useUiStore.getState().intakeDraft).toBe(false)
  })

  it('丢弃失败时卡片留着并报错', async () => {
    useUiStore.setState({ intakeDraft: true, projectRoot: '/p/e' })
    vi.spyOn(api, 'intakeDraft').mockResolvedValue('## Purpose\nx\n')
    vi.spyOn(api, 'discardIntakeDraft').mockRejectedValue(new Error('boom'))
    const toast = vi.spyOn(useUiStore.getState(), 'pushToast')
    const { el } = await render(<IntakeCard />)
    await act(async () => { button(el, 'Not now').dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.querySelector('[data-testid=intake-card]')).not.toBeNull()
    expect(toast).toHaveBeenCalled()
  })
})
