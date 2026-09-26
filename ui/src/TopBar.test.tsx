import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { TopBar } from './components/TopBar'
import { api } from './api'
import { useUiStore } from './store'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

describe('TopBar 流程入口（ADR 0069）', () => {
  beforeEach(() => {
    useUiStore.setState({
      projectName: 'p', packName: '规格驱动', mode: 'pack',
      pending: [], team: [], railOpen: false,
    })
    vi.spyOn(api, 'sandboxStatus').mockResolvedValue({ available: false, mode: 'none', note: '' })
  })
  afterEach(() => vi.restoreAllMocks())

  it('没有自治档下拉；点流程名弹出只读流程', async () => {
    const { el, root } = await render(<TopBar onSettings={() => {}} onProjectClosed={() => {}} />)
    expect(el.querySelector('select')).toBeNull()
    const flow = el.querySelector('[data-view-flow]') as HTMLButtonElement
    expect(flow.textContent).toContain('规格驱动')
    await act(async () => { flow.click() })
    const dialog = el.querySelector('[data-flow-dialog]')
    expect(dialog).toBeTruthy()
    expect(dialog?.textContent).toContain('规格')
    expect(dialog?.textContent).toContain('产品策划')
    expect(useUiStore.getState().railOpen).toBe(false)
    root.unmount()
  })
  it('无沙箱时提示停止执行，不再承诺裸跑审批兜底', async () => {
    const previous = i18n.language
    await i18n.changeLanguage('zh-CN')
    const { el, root } = await render(<TopBar onSettings={() => {}} onProjectClosed={() => {}} />)
    try {
      const badge = Array.from(el.querySelectorAll('[title]')).find(node => node.textContent?.includes('无沙箱'))
      expect(badge?.getAttribute('title')).toContain('停止终端执行')
      expect(badge?.getAttribute('title')).not.toContain('裸跑')
    } finally {
      await act(async () => { root.unmount() })
      el.remove()
      await i18n.changeLanguage(previous)
    }
  })

})
