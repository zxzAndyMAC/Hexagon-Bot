// 原 SidePanel.test（ADR 0088：右栏五页签撤掉）。断言迁到新落点并注明原因：
// - 产物长名 / 待恢复：右栏「产物」页签 → 画布的「产物」清单页签（侧栏「产物」打开；票 03 起是真画布）。
// - 阶段进度：右栏「阶段」页签 → 侧栏「流程」分组常驻（B2，只读，不再是单独页签）。
// 另加侧栏本身（B1/B3/B4）的行为：待决徽标口径、团队行状态、用量入口三档。
import { describe, expect, it, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
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

    // 票 09（B7 清单页签换皮）：内联样式换成 Tailwind 类——行 title 给全名，路径 min-w-0 + truncate，
    // 胶囊 flex-none（A4 Badge 自带 whitespace-nowrap）。断言意图不变：长名截断、胶囊不被挤成竖排。
    const item = canvas.querySelector('[role=listitem]') as HTMLElement
    expect(item.getAttribute('title')).toBe(path)
    const name = [...item.querySelectorAll('span')].find((s) => s.textContent === path) as HTMLElement
    expect(name.className).toContain('min-w-0')
    expect(name.className).toContain('truncate')
    const chip = item.querySelector('[data-slot="badge"]') as HTMLElement
    expect(chip.className).toContain('flex-none')
    expect(chip.className).toContain('whitespace-nowrap')
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

  // owner-followups-1015 #02 回归（2026-10-15 负责人：深浅两套下阶段圆圈粗细不匀）：
  // 曾是 15px 圆点 + 1.5px 内环（+ 1.5px 连线在左 6.5）。实测（Playwright，沿圆周每 3° 径向积分）
  // 描边视觉粗细的相对极差 DPR1 ≈ 25%、DPR2 ≈ 15%；16px + 2px 降到 ≈ 17% / 8%，贴近 SVG 抗锯齿底噪（13% / 9%）。
  // 守住：圆点、环、连线都是整数像素，偶数圆点让行内居中（28 高）落整数，连线中心对准圆心。
  it('阶段圆点 / 环 / 连线都是整数像素，连线居中对准圆心（#02）', async () => {
    const all: StageRow['state'][] = ['done', 'active', 'waiting_stamp', 'interrupted', 'rejected', 'skipped', 'pending']
    useUiStore.setState({ stages: all.map((state, i) => ({ run_id: `r${i}`, stage: state, seq: i, state })) })
    const host = await render(sidebar)
    const px = (cls: string, re: RegExp) => { const m = cls.match(re); expect(m, `${re} in ${cls}`).toBeTruthy(); return Number(m![1]) }
    const int = (n: number) => expect(Number.isInteger(n), `${n}px 不是整数像素`).toBe(true)
    for (const li of host.querySelectorAll<HTMLElement>('[data-stage-list] li')) {
      const dot = li.querySelector<HTMLElement>('span.rounded-full')!
      const size = px(dot.className, /size-\[([\d.]+)px\]/)
      int(size)
      expect(size % 2, '偶数圆点：28 高行内居中落整数').toBe(0)
      if (li.dataset.stageState !== 'done') int(px(dot.className, /inset_0_0_0_([\d.]+)px/))
      const link = li.querySelector<HTMLElement>('span[aria-hidden]')
      if (link) {
        const w = px(link.className, /w-\[([\d.]+)px\]/), left = px(link.className, /left-\[([\d.]+)px\]/)
        int(w); int(left)
        expect(left + w / 2).toBe(size / 2)
      }
    }
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

  // 行为变更（owner-followups-1015 票 09，foundations「对比度」）：休眠行原先整行 opacity .5，名字与「休眠」
  // 跟着掉到 AA 以下；改为只有头像半透明去饱和，名字照常 --ink。
  it('工作中显示像素格，休眠只淡化头像并写「休眠」；点成员打开该 Agent', async () => {
    useUiStore.setState({
      team: [
        { id: 'a1', role: '前端', model_slot: null, status: 'active', avatar_hash: null, avatar_override: false, model: null, duty: null, superior: null },
        { id: 'a2', role: 'QA', model_slot: null, status: 'sleeping', avatar_hash: null, avatar_override: false, model: null, duty: null, superior: null },
        { id: 'a3', role: '后端', model_slot: null, status: 'active', avatar_hash: null, avatar_override: false, model: null, duty: null, superior: null },
      ],
      streams: { a1: { 0: '正在写' } },
    })
    const host = await render(sidebar)
    const fe = host.querySelector('[data-member="a1"]') as HTMLButtonElement
    expect(fe.getAttribute('data-member-state')).toBe('working')
    expect(fe.querySelector('[data-slot="pixel-grid"]')).toBeTruthy()
    const qa = host.querySelector('[data-member="a2"]') as HTMLButtonElement
    expect(qa.getAttribute('data-member-state')).toBe('sleeping')
    expect(qa.className).not.toContain('opacity-50')
    expect(qa.textContent).toContain(i18n.t('sidebar.sleeping'))
    const face = qa.querySelector('[data-slot="avatar"]') as HTMLElement
    expect(face.getAttribute('data-dimmed')).toBe('true')
    expect(face.className).toContain('opacity-50')
    expect(face.className).toContain('grayscale')
    expect(fe.querySelector('[data-slot="avatar"]')!.hasAttribute('data-dimmed')).toBe(false)
    expect(host.querySelector('[data-member="a3"]')!.getAttribute('data-member-state')).toBe('online')
    await act(async () => { fe.click() })
    expect(useUiStore.getState().activeTab).toBe('agent:a1')
  })
})

// gap B6（2026-10-15）：成员悬停名片由过渡期 Tooltip「角色 · 模型」换成 B3 写的 HoverCard
// （模型 / 上级 / 职责）；聚焦也打开，键盘可读。
describe('成员悬停名片（B3 HoverCard）', () => {
  it('聚焦成员行出名片：模型、上级、职责；无上级写直接向你汇报', async () => {
    await i18n.changeLanguage('zh-CN')
    useUiStore.setState({
      team: [
        { id: 'a1', role: '前端', model_slot: 'code', status: 'active', avatar_hash: null, avatar_override: false, model: 'Claude Sonnet 5.5', duty: '实现界面与前端逻辑', superior: '架构师' },
        { id: 'a2', role: '项目经理', model_slot: null, status: 'active', avatar_hash: null, avatar_override: false, model: null, duty: null, superior: null },
      ],
      streams: {},
    })
    const host = await render(sidebar)
    const open = async (id: string) => {
      await act(async () => { (host.querySelector(`[data-member="${id}"]`) as HTMLButtonElement).focus() })
      await act(async () => { await new Promise((r) => setTimeout(r, 400)) })
      return document.querySelector(`[data-member-card="${id}"]`) as HTMLElement | null
    }
    const fe = await open('a1')
    expect(fe).toBeTruthy()
    expect(fe!.textContent).toContain('Claude Sonnet 5.5')
    expect(fe!.textContent).toContain('架构师')
    expect(fe!.textContent).toContain('实现界面与前端逻辑')
    await act(async () => { (host.querySelector('[data-member="a1"]') as HTMLButtonElement).blur() })
    const pm = await open('a2')
    expect(pm).toBeTruthy()
    expect(pm!.textContent).toContain('无（直接向你汇报）')
    expect(pm!.textContent).not.toContain('职责')
    await i18n.changeLanguage('en')
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
