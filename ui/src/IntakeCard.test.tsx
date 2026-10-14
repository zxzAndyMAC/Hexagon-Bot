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
    const confirm = vi.spyOn(api, 'confirmIntakeBrief').mockResolvedValue(undefined)
    const { el } = await render(<IntakeCard />)
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    const write = button(el, '写入 AGENTS.md')
    expect(write.dataset.variant).toBe('primary')
    expect(button(el, '先不写').dataset.variant).toBe('quiet')
    // 编辑没有 core 命令支撑：禁用但仍可聚焦（components §0）
    expect(button(el, '编辑').getAttribute('aria-disabled')).toBe('true')
    await act(async () => { write.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(confirm).toHaveBeenCalledOnce()
    expect(useUiStore.getState().intakeDraft).toBe(false)
    await i18n.changeLanguage('en')
  })

  it('先不写只在本次收起卡片，不调写入', async () => {
    useUiStore.setState({ intakeDraft: true, projectRoot: '/p/b' })
    const confirm = vi.spyOn(api, 'confirmIntakeBrief').mockResolvedValue(undefined)
    const { el } = await render(<IntakeCard />)
    await act(async () => { button(el, 'Not now').dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el.querySelector('[data-testid=intake-card]')).toBeNull()
    expect(confirm).not.toHaveBeenCalled()
    expect(useUiStore.getState().intakeDraft).toBe(true)
  })
})
