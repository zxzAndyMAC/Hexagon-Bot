import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
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
})
