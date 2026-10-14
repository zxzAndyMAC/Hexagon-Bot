// 原 SidePanel.test（ADR 0088：右栏五页签撤掉）。断言迁到新落点并注明原因：
// - 产物长名 / 待恢复：右栏「产物」页签 → 画布的「产物」清单页签（侧栏「产物」打开；票 03 起是真画布）。
// - 阶段进度：右栏「阶段」页签 → 侧栏「流程」分组常驻（B2，只读，不再是单独页签）。
// 另加侧栏本身（B1/B3/B4）的行为：待决徽标口径、团队行状态、用量入口三档。
import { describe, expect, it, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import './i18n'
import { Sidebar } from './components/shell/Sidebar'
import { Canvas } from './components/shell/Canvas'
import { TooltipProvider } from './components/ui/tooltip'
import { useUiStore } from './store'
import type { ArtifactRow } from './gen/ArtifactRow'
import type { StageRow } from './gen/StageRow'
import type { PendingQuestion } from './api'

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
const sidebar = <Sidebar onSearch={() => {}} onSettings={() => {}} onProjectClosed={() => {}} />
// 票 03：画布占位换成真画布（B7），宿主按 store 的 canvasOpen 显隐
function CanvasHost() {
  const open = useUiStore((s) => s.canvasOpen)
  return <Canvas open={open} mode="side" width={600} />
}

const snap = useUiStore.getState()
afterEach(async () => {
  await act(async () => root?.unmount())
  el?.remove()
  useUiStore.setState(snap, true)
})

// 回归：产物长名曾把右侧状态 chip 挤成竖排（flex 默认可缩 + chip 无 nowrap）。
// 钉住：路径行 title 给全名 + minWidth:0 截断，chip flexShrink:0+nowrap 保形。
describe('产物清单长名（ADR 0088：右栏页签 → 侧栏「产物」打开的画布占位）', () => {
  it('侧栏「产物」打开清单；路径截断、chip 不缩、悬浮 title 给全名', async () => {
    const path = '.hexagon/specs/web-product-requirements-document-v2-final.md'
    const row: ArtifactRow = {
      id: 'a1', path, kind: 'doc', tier: 'freeform', stage_run_id: null,
      author: null, version: 1, status: 'superseded', upstream_id: null,
    }
    useUiStore.setState({ artifacts: [row], canvasOpen: false, tabs: [], activeTab: null })
    const host = await render(<>{sidebar}<CanvasHost /></>)
    const nav = host.querySelector('[data-nav="artifacts"]') as HTMLButtonElement
    expect(nav.textContent).toContain('1')
    await act(async () => { nav.click() })
    expect(nav.getAttribute('aria-current')).toBe('true')
    // 票 03（ADR 0088）：画布占位撤掉，「产物」是画布里的清单页签
    expect(useUiStore.getState().activeTab).toBe('artifacts')
    const canvas = host.querySelector('[data-canvas] [role=tabpanel]:not([hidden])')!

    const name = canvas.querySelector('span.mono[title]') as HTMLElement
    expect(name.getAttribute('title')).toBe(path)
    expect(name.style.minWidth).toBe('0')

    const chip = canvas.querySelector('.chip') as HTMLElement
    expect(chip.style.flexShrink).toBe('0')
    expect(chip.style.whiteSpace).toBe('nowrap')
  })

  it('materialization conflicts remain visible as pending recovery', async () => {
    const row: ArtifactRow = Object.assign({
      id: 'pending-art', path: 'notes.md', kind: 'doc', tier: 'freeform' as const,
      stage_run_id: null, author: null, version: 2, status: 'pending' as const, upstream_id: null,
    }, { materialization: 'pending_recovery' })
    useUiStore.setState({ artifacts: [row], tabs: [], activeTab: null })
    useUiStore.getState().openCanvas('artifacts')
    const host = await render(<CanvasHost />)
    expect(host.textContent).toMatch(/Pending recovery|待恢复/)
  })
})

describe('阶段进度（ADR 0069 只读；ADR 0088：右栏「阶段」页签 → 侧栏常驻）', () => {
  it('侧栏「流程」分组直接列出阶段，没有退回 / 跳过 / 盖章按钮', async () => {
    const stages: StageRow[] = [
      { run_id: 'r1', stage: '规格', seq: 0, state: 'done' },
      { run_id: 'r2', stage: '实现', seq: 1, state: 'active' },
      { run_id: 'r3', stage: '测试', seq: 2, state: 'waiting_stamp' },
    ]
    useUiStore.setState({ stages })
    const host = await render(sidebar)
    const list = host.querySelector('[data-stage-list]')!
    expect(list.textContent).toContain('规格')
    expect(list.textContent).toContain('实现')
    expect(list.querySelector('button')).toBeNull()
    expect(list.querySelector('[data-stage-state="active"]')).toBeTruthy()
    expect(list.querySelector('[data-stage-state="waiting_stamp"]')).toBeTruthy()
  })
})

const card = (id: string): PendingQuestion => ({ id, kind: 'permission', agent_id: 'a1', payload: {}, state: 'queued' } as PendingQuestion)

describe('侧栏待决与团队（B1、B3）', () => {
  it('待决徽标 = 待决卡 + 待复审提案；为 0 时不出徽标；状态读取失败时不显示数字', async () => {
    useUiStore.setState({ pending: [card('q1'), card('q2')], reviewRows: [{ id: 'r1' } as never] })
    const host = await render(sidebar)
    expect(host.querySelector('[data-pending-count]')?.getAttribute('data-pending-count')).toBe('3')
    await act(async () => useUiStore.setState({ statusError: true }))
    expect(host.querySelector('[data-pending-count]')).toBeNull()
    expect(host.querySelector('[data-status-error]')).toBeTruthy()
    await act(async () => useUiStore.setState({ statusError: false, pending: [], reviewRows: [] }))
    expect(host.querySelector('[data-pending-count]')).toBeNull()
  })

  it('点「待决」打开现有待决界面', async () => {
    useUiStore.setState({ pending: [card('q1')], pendingDialogOpen: false })
    const host = await render(sidebar)
    await act(async () => { (host.querySelector('[data-nav="pending"]') as HTMLButtonElement).click() })
    expect(useUiStore.getState().pendingDialogOpen).toBe(true)
  })

  it('工作中显示像素格，休眠半透明并写「休眠」；点成员打开该 Agent', async () => {
    useUiStore.setState({
      team: [
        { id: 'a1', role: '前端', model_slot: null, status: 'active', avatar_hash: null },
        { id: 'a2', role: 'QA', model_slot: null, status: 'sleeping', avatar_hash: null },
        { id: 'a3', role: '后端', model_slot: null, status: 'active', avatar_hash: null },
      ],
      streams: { a1: { 0: '正在写' } },
    })
    const host = await render(sidebar)
    const fe = host.querySelector('[data-member="a1"]') as HTMLButtonElement
    expect(fe.getAttribute('data-member-state')).toBe('working')
    expect(fe.querySelector('[data-slot="pixel-grid"]')).toBeTruthy()
    const qa = host.querySelector('[data-member="a2"]') as HTMLButtonElement
    expect(qa.getAttribute('data-member-state')).toBe('sleeping')
    expect(qa.className).toContain('opacity-50')
    expect(host.querySelector('[data-member="a3"]')!.getAttribute('data-member-state')).toBe('online')
    await act(async () => { fe.click() })
    expect(useUiStore.getState().activeTab).toBe('agent:a1')
  })
})

describe('用量入口三档（B4，usage_summary）', () => {
  const total = (spent_mc: number, limit_cents: number | null) => ({
    reserved_mc: 0, spent_mc, limit_cents, tokens: 0, unknown_requests: 0, legacy_unknown_records: 0, unknown_token_records: 0,
  })
  it('ok 只图标、warn 出百分比、cap 出「上限」，可读名随档位', async () => {
    useUiStore.setState({ usageTotal: total(3_860_000, 10000) })
    const host = await render(sidebar)
    const btn = () => host.querySelector('[data-usage-tier]') as HTMLButtonElement
    expect(btn().getAttribute('data-usage-tier')).toBe('ok')
    expect(btn().textContent).toBe('')
    expect(btn().getAttribute('aria-label')).toMatch(/38/)
    await act(async () => useUiStore.setState({ usageTotal: total(8_600_000, 10000) }))
    expect(btn().getAttribute('data-usage-tier')).toBe('warn')
    expect(btn().textContent).toBe('86%')
    await act(async () => useUiStore.setState({ usageTotal: total(10_000_000, 10000) }))
    expect(btn().getAttribute('data-usage-tier')).toBe('cap')
    expect(btn().textContent).toMatch(/Cap|上限/)
  })
})
