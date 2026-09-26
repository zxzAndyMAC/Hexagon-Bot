import { describe, expect, it, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import './i18n'
import { UsageTab } from './components/UsageTab'
import { useUiStore } from './store'
import { api } from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

describe('UsageTab 撞限压力行（context-window 票 03 / ADR 0068）', () => {
  const snap = useUiStore.getState()
  let root: Root | undefined
  let el: HTMLElement | undefined

  afterEach(() => {
    act(() => { root?.unmount() })
    el?.remove()
    useUiStore.setState(snap, true)
    vi.restoreAllMocks()
  })

  it('显示 14 天撞限卡与机械压缩计数 + 恢复层阈值说明', async () => {
    vi.spyOn(api, 'usageSeries').mockResolvedValue([])
    useUiStore.setState({
      contextPressure: { overflow_cards_14d: 2, compactions_14d: 5 },
      usageRows: [],
      usageTotal: null,
      team: [],
    })
    ;({ el, root } = await render(<UsageTab />))
    const text = el!.textContent ?? ''
    // 两个计数成行渲染
    expect(text).toMatch(/Overflow cards\s*2/)
    expect(text).toMatch(/Compactions\s*5/)
    // 阈值说明：≥2 触发恢复设计票
    expect(text).toContain('two weeks')
  })

  it('contextPressure 为 null（空项目/未拉到）→ 计数显示 0', async () => {
    vi.spyOn(api, 'usageSeries').mockResolvedValue([])
    useUiStore.setState({ contextPressure: null, usageRows: [], usageTotal: null, team: [] })
    ;({ el, root } = await render(<UsageTab />))
    const text = el!.textContent ?? ''
    expect(text).toMatch(/Overflow cards\s*0/)
    expect(text).toMatch(/Compactions\s*0/)
  })
  it.each([null, 100])('未知费用持续显示，预算为 %s 时也不伪装成免费', async (limit) => {
    vi.spyOn(api, 'usageSeries').mockResolvedValue([])
    useUiStore.setState({ usageRows: [], team: [], usageTotal: {
      spent_mc: 0, reserved_mc: 0, limit_cents: limit, tokens: 0, unknown_requests: 1, legacy_unknown_records: 0, unknown_token_records: 1,
    } })
    ;({ el, root } = await render(<UsageTab />))
    expect(el!.textContent).toContain('Unknown cost')
    expect(el!.textContent).toContain('Unknown token usage')
    if (limit !== null) expect(el!.textContent).toContain('monetary budget cannot be guaranteed')
  })

  it('在途预占单列为估算，不混入已知金额', async () => {
    vi.spyOn(api, 'usageSeries').mockResolvedValue([])
    useUiStore.setState({ usageRows: [], team: [], usageTotal: {
      spent_mc: 0, reserved_mc: 120000, limit_cents: 1000, tokens: 0,
      unknown_requests: 1, legacy_unknown_records: 0, unknown_token_records: 1,
    } })
    ;({ el, root } = await render(<UsageTab />))
    expect(el!.textContent).toContain('In-flight estimate')
    expect(el!.textContent).toContain('¥1.20')
  })

})
