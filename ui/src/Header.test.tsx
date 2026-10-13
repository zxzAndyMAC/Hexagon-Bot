// 原 TopBar.test（ADR 0088：顶栏撤掉）。断言迁到新落点并注明原因：
// - 流程入口：顶栏流程包 chip → 侧栏「流程」分组的「查看流程」与头部 ⋯ 菜单「查看流程」（components B1、B5）。
// - 沙箱状态：顶栏徽标 → 头部 ⋯ 菜单脚注（B5 补设计，票 02）。文案断言不变。
import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { Header } from './components/shell/Header'
import { Sidebar } from './components/shell/Sidebar'
import { FlowDialog } from './components/shell/FlowDialog'
import { TooltipProvider } from './components/ui/tooltip'
import { api } from './api'
import { useUiStore } from './store'
import type { StageRow } from './gen/StageRow'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: Root | undefined
let el: HTMLDivElement | undefined
async function render(node: React.ReactNode) {
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => { root!.render(<TooltipProvider>{node}</TooltipProvider>) })
  return el
}
/** Radix DropdownMenu 在 pointerdown（非 click）时打开。 */
async function openMenu(trigger: HTMLElement) {
  await act(async () => {
    trigger.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, button: 0, ctrlKey: false, pointerType: 'mouse' }))
  })
}

const snap = useUiStore.getState()
beforeEach(() => {
  useUiStore.setState({ projectName: 'p', packName: '规格驱动', mode: 'pack', pending: [], reviewRows: [], team: [], stages: [], flowDialogOpen: false })
  vi.spyOn(api, 'sandboxStatus').mockResolvedValue({ available: false, mode: 'none', note: '' })
  vi.spyOn(api, 'projectDelegation').mockResolvedValue({ enabled: false, paused: false, generation: 0, delegation_id: null })
})
afterEach(async () => {
  await act(async () => root?.unmount())
  el?.remove()
  document.body.replaceChildren()
  vi.restoreAllMocks()
  useUiStore.setState(snap, true)
})

describe('流程入口（ADR 0069；ADR 0088 迁到侧栏与头部 ⋯）', () => {
  it('侧栏项目块副标是流程包；「查看流程」弹出只读流程，没有自治档下拉', async () => {
    const host = await render(<><Sidebar onSearch={() => {}} onSettings={() => {}} onProjectClosed={() => {}} /><FlowDialog /></>)
    expect(host.querySelector('select')).toBeNull()
    expect(host.querySelector('[data-project-block]')?.textContent).toContain('规格驱动')
    const view = host.querySelector('button[aria-label="View flow"], button[aria-label="查看流程"]') as HTMLButtonElement
    await act(async () => { view.click() })
    const dialog = host.querySelector('[data-flow-dialog]')
    expect(dialog).toBeTruthy()
    expect(dialog?.textContent).toContain('规格')
    expect(dialog?.textContent).toContain('产品策划')
    // 查看流程不动侧栏收起状态
    expect(useUiStore.getState().sideLayout.collapsed).toBe(false)
  })

  it('头部 ⋯ 菜单也能打开只读流程', async () => {
    const host = await render(<><Header /><FlowDialog /></>)
    await openMenu(host.querySelector('[data-header-more]') as HTMLElement)
    const item = [...document.querySelectorAll('[role="menuitem"]')].find((n) => /View flow|查看流程/.test(n.textContent ?? '')) as HTMLElement
    await act(async () => { item.click() })
    expect(useUiStore.getState().flowDialogOpen).toBe(true)
  })
})

describe('沙箱状态（票 06；ADR 0088 迁到头部 ⋯ 菜单脚注）', () => {
  it('无沙箱时提示停止执行，不再承诺裸跑审批兜底', async () => {
    const previous = i18n.language
    await i18n.changeLanguage('zh-CN')
    try {
      const host = await render(<Header />)
      await act(async () => { await Promise.resolve() })
      await openMenu(host.querySelector('[data-header-more]') as HTMLElement)
      const badge = document.querySelector('[data-sandbox]') as HTMLElement
      expect(badge.textContent).toContain('无沙箱')
      expect(badge.getAttribute('title')).toContain('停止终端执行')
      expect(badge.getAttribute('title')).not.toContain('裸跑')
    } finally {
      await i18n.changeLanguage(previous)
    }
  })
})

describe('头部 crumb 与画布开关（B5）', () => {
  const stages: StageRow[] = [
    { run_id: 'r1', stage: '规格', seq: 0, state: 'done' },
    { run_id: 'r2', stage: '实现', seq: 1, state: 'active' },
    { run_id: 'r3', stage: '测试', seq: 2, state: 'pending' },
  ]
  it('显示当前阶段与「阶段 n / N」；阶段未知时显示项目名、无胶囊', async () => {
    useUiStore.setState({ stages })
    const host = await render(<Header />)
    expect(host.querySelector('[data-header]')!.textContent).toContain('实现')
    expect(host.querySelector('[data-stage-chip]')!.textContent).toMatch(/2 \/ 3/)
    await act(async () => useUiStore.setState({ stages: [] }))
    expect(host.querySelector('[data-header]')!.textContent).toContain('p')
    expect(host.querySelector('[data-stage-chip]')).toBeNull()
  })

  it('画布开关 tooltip 带 ⌥⌘B 绑定，开时 aria-pressed', async () => {
    const host = await render(<Header />)
    const btn = host.querySelector('[data-header-canvas]') as HTMLButtonElement
    expect(btn.getAttribute('aria-pressed')).toBe('false')
    await act(async () => { btn.click() })
    expect(useUiStore.getState().canvasOpen).toBe(true)
    expect(btn.getAttribute('aria-pressed')).toBe('true')
  })

  it('没有 Agent Screen 目标时不渲染「屏幕」；有目标时点它发切换请求', async () => {
    const host = await render(<Header />)
    expect(host.querySelector('[data-header-screen]')).toBeNull()
    await act(async () => useUiStore.setState({ agentScreenAvailable: true }))
    const before = useUiStore.getState().agentScreenReq
    await act(async () => { (host.querySelector('[data-header-screen]') as HTMLButtonElement).click() })
    expect(useUiStore.getState().agentScreenReq).toBe(before + 1)
  })
})
