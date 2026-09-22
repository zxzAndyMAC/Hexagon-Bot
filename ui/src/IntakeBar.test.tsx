import { describe, expect, it, vi, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import i18n from './i18n'
import { IntakeBar } from './components/IntakeBar'
import { api } from './api'
import { bindingFor, formatBinding } from './keymap'
import { useUiStore } from './store'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

afterEach(() => {
  document.body.innerHTML = ''
  useUiStore.setState({ intakeDraft: false })
  vi.restoreAllMocks()
})

describe('开场草案条（票 17）', () => {
  it('没有草案时不占位', async () => {
    useUiStore.setState({ intakeDraft: false })
    const { el } = await render(<IntakeBar />)
    expect(el.querySelector('[data-testid=intake-bar]')).toBeNull()
  })

  it('草案条不是弹窗，输入框仍可打字，tooltip 显示当前绑定', async () => {
    await i18n.changeLanguage('zh-CN')
    useUiStore.setState({ intakeDraft: true })
    const confirm = vi.spyOn(api, 'confirmIntakeBrief').mockResolvedValue(undefined)
    const { el } = await render(
      <>
        <IntakeBar />
        <textarea data-testid="composer-input" />
      </>,
    )
    expect(el.querySelector('[role="dialog"]')).toBeNull()
    const ta = el.querySelector('textarea') as HTMLTextAreaElement
    expect(ta.disabled).toBe(false)
    const btn = el.querySelector('button') as HTMLButtonElement
    expect(btn.textContent).toContain('写入项目说明')
    expect(btn.title).toContain(formatBinding(bindingFor('confirmIntake')))
    await act(async () => {
      btn.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    expect(confirm).toHaveBeenCalledOnce()
    expect(useUiStore.getState().intakeDraft).toBe(false)
    await i18n.changeLanguage('en')
  })
})
