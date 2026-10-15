// App 级用量行为（ui-redesign 票 12；components B4 / D20 / E12 / E13 / A23，interaction §10）。
// 入口点击弹用量卡（不再直接开详情）→「详情」进入占满主区的用量详情；触顶版「提高上限」去设置「用量」分区；
// 详情的粒度 / 时间范围 / 刷新都按 UTC 日界重新读 usage_series；加载失败给重试，空范围给空态。
// 图表已从 ECharts 换成 Recharts：本文件不再需要 echarts 桩。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { api, type UsageBucket } from './api'
import { useUiStore } from './store'
import i18n from './i18n'
import { daysAgo } from './usage'

// 票 04（ADR 0089）：待决弹窗 PendingDialog 换成待决坞 PendingDock，桩随之改名
vi.mock('./components/PendingDock', () => ({ PendingDock: () => null }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
let series: ReturnType<typeof vi.spyOn>
const initial = useUiStore.getState()

const total = (spent_mc: number, tokens = 0) => ({
  reserved_mc: 0, spent_mc, limit_cents: 10000, tokens, unknown_requests: 0, legacy_unknown_records: 0, unknown_token_records: 0,
})
const bucket = (b: string, agent: string, p: number): UsageBucket =>
  ({ bucket: b, agent_id: agent, prompt_tokens: p, completion_tokens: 0, tool_output_tokens: 0, cost_mc: 0 })
const q = <T extends Element = HTMLElement>(s: string) => (document.querySelector(s) as T | null)
const btn = (scope: ParentNode, text: string | RegExp) =>
  [...scope.querySelectorAll('button')].find((b) => (typeof text === 'string' ? b.textContent === text : text.test(b.textContent ?? ''))) as HTMLButtonElement
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })

beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({
    projectRoot: null, projectGeneration: null, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null,
    team: [{ id: 'a1', role: 'Frontend', model_slot: null, status: 'active' }, { id: 'a2', role: 'Backend', model_slot: null, status: 'sleeping' }] as never,
    pending: [], timeline: [], modalScope: 'workbench', intakeDraft: false, sideLayout: { width: 252, collapsed: false },
    usageTotal: total(3_860_000, 1_240_000), usageRows: [], usageSeries7d: [], contextPressure: { overflow_cards_14d: 2, compactions_14d: 5 }, confirmReq: null,
  })
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue()
  series = vi.spyOn(api, 'usageSeries').mockResolvedValue([])
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => root.render(<App />))
})

afterEach(async () => {
  await act(async () => root.unmount())
  el.remove()
  vi.restoreAllMocks()
  useUiStore.setState(initial)
})

const openCard = async () => {
  await act(async () => { (el.querySelector('[data-usage-tier]') as HTMLButtonElement).click() })
  return q('[data-usage-popover]')!
}

describe('用量入口 → 用量卡（D20，usage#quick）', () => {
  it('点击入口弹出用量卡而不是直接进详情；「详情」进入占满主区的用量详情并收起卡', async () => {
    const card = await openCard()
    // 票 02 过渡态（入口直接开详情）在票 12 结束：先弹卡，详情视图不动
    expect(useUiStore.getState().usageView).toBe(false)
    expect(card.querySelector('[data-usage-card="ok"]')).toBeTruthy()
    expect(card.textContent).toContain('This month')
    expect(card.textContent).toMatch(/Overflow cards · 14 days\s*2/)
    expect(card.textContent).toMatch(/Compactions\s*5/)
    expect(card.textContent).toContain('1.24M tokens')
    await act(async () => { (card.querySelector('[data-usage-detail]') as HTMLButtonElement).click() })
    expect(useUiStore.getState().usageView).toBe(true)
    expect(el.querySelector('[data-usage-view]')).toBeTruthy()
    expect(q('[data-usage-popover]')).toBeNull()
  })

  it('触顶版：红头 + 「提高上限」打开设置的用量分区', async () => {
    await act(async () => useUiStore.setState({ usageTotal: total(10_000_000) }))
    const card = await openCard()
    expect(card.querySelector('[data-usage-card="cap"]')?.textContent).toContain('Usage cap reached')
    await act(async () => { btn(card, 'Raise cap').click() })
    await flush()
    expect(q('[data-usage-popover]')).toBeNull()
    expect(q('[data-usage-settings]')).toBeTruthy()
  })
})

describe('用量详情（E12 / E13 / A23，usage#detail）', () => {
  const openDetail = async () => {
    await act(async () => useUiStore.getState().openUsageDetail())
    await flush()
    return el.querySelector('[data-usage-view]') as HTMLElement
  }

  it('默认近 30 天按天读取（UTC 日界），并读等长上一周期算环比', async () => {
    series.mockImplementation(async (_g: string, from?: string | null) =>
      from === daysAgo(29) ? [bucket(daysAgo(1), 'a1', 1180), bucket(daysAgo(1), 'a2', 0)] : [bucket(daysAgo(40), 'a1', 1000)])
    const view = await openDetail()
    expect(series).toHaveBeenCalledWith('day', daysAgo(29), daysAgo(0))
    expect(series).toHaveBeenCalledWith('day', daysAgo(59), daysAgo(30))
    const totalCard = view.querySelector('[data-stat="total"]')!
    expect(totalCard.textContent).toContain('1.18K')
    expect(totalCard.textContent).toMatch(/↑ 18% vs previous period/)
    expect(view.querySelector('[data-stat="agents"]')?.textContent).toMatch(/1\s*\/ 2.*1 asleep/)
    expect(view.querySelector('[data-usage-trend] [data-area-trend="stacked"]')).toBeTruthy()
    expect(view.textContent).toContain('days follow UTC')
  })

  it('粒度切到按小时、预设改近 7 天、刷新，都会重新读取', async () => {
    const view = await openDetail()
    series.mockClear()
    const hour = btn(view.querySelector('[data-usage-granularity]')!, 'By hour')
    await act(async () => { hour.click() })
    await flush()
    expect(series).toHaveBeenCalledWith('hour', daysAgo(29), daysAgo(0))

    await act(async () => { (view.querySelector('[data-usage-range="header"]') as HTMLButtonElement).click() })
    const pop = q('[data-range-pop]')!
    expect(pop.querySelectorAll('[data-preset]')).toHaveLength(8)
    await act(async () => { (pop.querySelector('[data-preset="7d"]') as HTMLButtonElement).click() })
    await flush()
    expect(series).toHaveBeenCalledWith('hour', daysAgo(6), daysAgo(0))
    expect(view.querySelector('[data-usage-range="header"]')?.textContent).toContain('Last 7 days')

    series.mockClear()
    await act(async () => { (view.querySelector('[data-usage-refresh]') as HTMLButtonElement).click() })
    await flush()
    expect(series).toHaveBeenCalledWith('hour', daysAgo(6), daysAgo(0))
    expect(useUiStore.getState().invalidate).toHaveBeenCalledWith('usage')
  })

  it('读取失败给 C5 + 重试；空范围给 A19 空态与「全部时间」', async () => {
    series.mockRejectedValue(new Error('boom'))
    const view = await openDetail()
    const err = view.querySelector('[data-usage-trend] [data-usage-error]') as HTMLElement
    expect(err.getAttribute('role')).toBe('alert')
    series.mockResolvedValue([])
    await act(async () => { btn(err, 'Retry').click() })
    await flush()
    const empty = view.querySelector('[data-usage-trend] [data-usage-empty]') as HTMLElement
    expect(empty.textContent).toContain('No usage in this period')
    series.mockClear()
    await act(async () => { btn(empty, 'All time').click() })
    await flush()
    expect(series).toHaveBeenCalledWith('day', null, null)
  })

  it('消费榜单：团队视图按 Agent 确定色排行；切到模型视图换环形图', async () => {
    series.mockResolvedValue([bucket(daysAgo(2), 'a1', 900), bucket(daysAgo(2), 'a2', 300)])
    await act(async () => useUiStore.setState({ usageRows: [
      { agent_id: 'a1', model: 'gpt-x', stage: 'spec', prompt_tokens: 900, completion_tokens: 0, tool_output_tokens: 0, cost_mc: 0, calls: 1, unknown_requests: 0, legacy_unknown_records: 0, unknown_token_records: 0 },
    ] }))
    const view = await openDetail()
    const rows = [...view.querySelectorAll('[data-usage-board="team"] [data-hbar]')]
    expect(rows.map((r) => r.textContent)).toEqual([expect.stringContaining('Frontend'), expect.stringContaining('Backend')])
    expect((rows[0].querySelector('i') as HTMLElement).style.background).toMatch(/--agent-c\d/)
    await act(async () => { btn(view.querySelector('[data-usage-board]')!, 'Model').click() })
    expect(view.querySelector('[data-model-donut]')?.textContent).toContain('gpt-x')
    expect(view.querySelector('[data-usage-stages]')?.textContent).toContain('spec')
  })
})
