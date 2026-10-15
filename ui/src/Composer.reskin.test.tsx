// ui-redesign 票 07（components D1–D5、D18、D19；interaction §3；ADR 0090）：输入条与命令面板换皮带来的新交互。
// 旧行为（原子块整块删除、120ms 防抖、乱序守卫、发送三态）仍由 Composer.test 守。
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { useUiStore } from './store'
import { Composer } from './components/Composer'
import { ContextMeter } from './components/ContextMeter'
import { ComputerTaskReadiness } from './components/ComputerTaskReadiness'
import { CommandPalette } from './components/CommandPalette'
import { mockTimelineFacts } from './timelineMock'
import type { TimelineItem } from './api'
import type { TeamRow } from './gen/TeamRow'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let el: HTMLDivElement
let root: Root
const member = (role: string, id = `id-${role}`, status: 'active' | 'sleeping' = 'active'): TeamRow =>
  ({ id, role, model_slot: 'chat', status, avatar_hash: null, model: null, duty: null })

async function mount(node: React.ReactNode) {
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => root.render(node))
}
async function type(body: string) {
  const ta = el.querySelector('textarea')!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(ta, body)
    ta.dispatchEvent(new Event('input', { bubbles: true }))
  })
  return ta
}

beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({ team: [member('QA'), member('Dev', 'b1', 'sleeping')], artifacts: [], pending: [], timeline: [], mcpPending: false, projectRoot: null })
})
afterEach(async () => {
  await act(async () => root?.unmount())
  el?.remove()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

describe('D1 输入条', () => {
  it('+ 附件打开图片选择器，选中的图片走同一条附件通道', async () => {
    const stage = vi.spyOn(api, 'stageAttachment').mockResolvedValue({ media_type: 'image/png', path: '.hexagon/inbox/p.png', bytes: 3, name: 'p.png' })
    await mount(<Composer />)
    const picker = el.querySelector<HTMLInputElement>('input[type=file]')!
    const click = vi.spyOn(picker, 'click')
    await act(async () => el.querySelector<HTMLButtonElement>('[aria-label="Attach images"]')!.click())
    expect(click).toHaveBeenCalledOnce()
    expect(picker.accept).toContain('image/png')
    const file = new File([new Uint8Array([1, 2, 3])], 'p.png', { type: 'image/png' })
    Object.defineProperty(picker, 'files', { value: [file], configurable: true })
    await act(async () => { picker.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(stage).toHaveBeenCalledWith('p.png', expect.any(Uint8Array))
    expect(el.querySelector('.attach-chip')).toBeTruthy()
  })

  it('发送中输入保持可编辑：发出去的那段清掉，期间新打的字留下', async () => {
    let finish!: (n: number) => void
    vi.spyOn(api, 'sendMessage').mockImplementation(() => new Promise((ok) => { finish = ok }))
    vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue()
    await mount(<Composer />)
    const ta = await type('first')
    await act(async () => ta.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    expect(ta.disabled).toBe(false)
    expect(el.querySelector('.composer-send')!.getAttribute('aria-busy')).toBe('true')
    await type('first second')
    await act(async () => finish(1))
    expect(ta.value).toBe('second')
  })

  it('网页引用说明只在有引用时出现', async () => {
    await mount(<Composer />)
    expect(el.querySelector('.composer-hint-foot')).toBeNull()
  })
})

describe('D2 建议弹层', () => {
  it('@ 弹层按 D2 画行：分组标题、多实例 [id]、休眠角色右侧「Asleep」', async () => {
    useUiStore.setState({ team: [member('QA'), member('Dev', 'b1', 'sleeping'), member('Dev', 'b2')] })
    await mount(<Composer />)
    await type('@')
    const list = el.querySelector('[role=listbox]')!
    expect(list.textContent).toContain('Mention a role')
    const rows = [...list.querySelectorAll<HTMLElement>('[role=option]')]
    expect(rows.map((r) => r.dataset.label)).toEqual(['@QA', '@Dev[b1]', '@Dev[b2]'])
    expect(rows[1].textContent).toContain('Dev [b1]')
    expect(rows[1].textContent).toContain('Asleep')
    expect(rows[2].textContent).not.toContain('Asleep')
  })

  it('# 首屏出骨架，防抖期间保持旧结果，产物路径标版本', async () => {
    vi.useFakeTimers()
    useUiStore.setState({ artifacts: [{ path: 'docs/plan.md', version: 2 } as never] })
    const paths = vi.spyOn(api, 'repoPaths').mockResolvedValue(['docs/', 'docs/plan.md'])
    await mount(<Composer />)
    await type('#d')
    expect(el.querySelectorAll('.composer-pop-skel')).toHaveLength(3)
    await act(async () => { await vi.advanceTimersByTimeAsync(130) })
    expect(paths).toHaveBeenCalledWith('d')
    expect(el.querySelector('[data-label="#docs/plan.md"]')!.textContent).toContain('Artifact v2')
    expect(el.querySelector('[data-label="#docs/"]')!.textContent).toContain('dir')
    await type('#do')
    // 防抖 120ms 内旧结果还在，没有闪成骨架
    expect(el.querySelector('[data-label="#docs/plan.md"]')).toBeTruthy()
    expect(el.querySelector('.composer-pop-skel')).toBeNull()
  })

  it('# 读取失败出红字与重试，重试再查同一个词', async () => {
    vi.useFakeTimers()
    const paths = vi.spyOn(api, 'repoPaths').mockRejectedValueOnce(new Error('io')).mockResolvedValue(['src/a.ts'])
    await mount(<Composer />)
    await type('#sr')
    await act(async () => { await vi.advanceTimersByTimeAsync(130) })
    const alert = el.querySelector('[role=listbox] [role=alert]')!
    expect(alert.textContent).toContain("Couldn't load paths")
    await act(async () => [...alert.querySelectorAll('button')].find((b) => b.textContent === 'Retry')!.click())
    await act(async () => { await vi.advanceTimersByTimeAsync(130) })
    expect(paths).toHaveBeenLastCalledWith('sr')
    expect(el.querySelector('[data-label="#src/a.ts"]')).toBeTruthy()
  })
})

describe('D5 上下文圆环', () => {
  const event = (used: number): TimelineItem => ({ message: null, event: {
    id: 1, project_id: 'p', agent_id: 'id-QA', stage_run_id: null, kind: 'system', created_at: '2026-10-01 06:00:00',
    payload: { kind: 'request_envelope', context: { used_tokens: used, window_tokens: 200_000, compact_at_tokens: 160_000, model_slot: 'chat' } },
  } })
  it('悬停卡写已用 / 窗口 / 压缩阈值；点击固定；到压缩阈值环转橙', async () => {
    useUiStore.setState({ timelineFacts: mockTimelineFacts([event(84_000)], { expected_project_root: '/p', watched_tool_streams: [], watched_message_ids: [] }) })
    await mount(<ContextMeter agentIds={['id-QA']} />)
    const trigger = el.querySelector<HTMLButtonElement>('.context-meter-trigger')!
    expect(trigger.textContent).toBe('42%')
    expect(el.querySelector('.cring')!.hasAttribute('data-over')).toBe(false)
    await act(async () => trigger.click())
    const card = el.querySelector('[role=tooltip]')!
    expect(card.textContent).toContain('QA · context')
    expect(card.textContent).toContain('84K')
    expect(card.textContent).toContain('200K')
    expect(trigger.getAttribute('aria-expanded')).toBe('true')
    await act(async () => useUiStore.setState({ timelineFacts: mockTimelineFacts([event(170_000)], { expected_project_root: '/p', watched_tool_streams: [], watched_message_ids: [] }) }))
    expect(el.querySelector('.cring')!.hasAttribute('data-over')).toBe(true)
  })
})

describe('D19 预检卡「打开模型服务」', () => {
  it('打开设置 › 模型服务；「检查角色模型」仍去团队', async () => {
    useUiStore.setState({ team: [member('QA')], projectRoot: '/project-a' })
    vi.spyOn(api, 'desktopStatus').mockResolvedValue({ project_root: '/project-a', enabled: true, active_project: null, active_agent: null, busy: false, paused: false, outcome_unknown: false, screenshot_count: 0, capture_preparation: 'ready' })
    vi.spyOn(api, 'computerModels').mockResolvedValue({ project_root: '/project-a', roles: [{ agent_id: 'id-QA', vision: 'no_vision', model: 'text' }], vision_slots: [] })
    const section = vi.fn()
    const onSection = (e: Event) => section((e as CustomEvent).detail)
    window.addEventListener('hexagon:open-settings-section', onSection)
    try {
      await mount(<ComputerTaskReadiness mentions={[]} />)
      await act(async () => el.querySelector<HTMLButtonElement>('[data-readiness-services]')!.click())
      expect(section).toHaveBeenCalledExactlyOnceWith('models')
      expect(el.querySelector('[data-readiness-models]')).toBeTruthy()
    } finally { window.removeEventListener('hexagon:open-settings-section', onSection) }
  })
})

describe('D18 命令面板', () => {
  it('导航组含侧栏 / 画布 / 全宽 / 专注 / 设置，不再有分屏；空结果写搜索词', async () => {
    useUiStore.setState({ artifacts: [], invalidate: async () => {} })
    const settings = vi.fn()
    const toggleCanvas = vi.fn()
    useUiStore.setState({ toggleCanvas })
    await mount(<CommandPalette onClose={() => {}} onSettings={settings} />)
    const labels = [...document.querySelectorAll('[data-command-palette] [role=group]')]
      .find((g) => g.getAttribute('aria-label') === 'Navigation')
    const text = labels?.textContent ?? document.querySelector('[data-command-palette]')!.textContent!
    for (const want of ['Toggle canvas', 'Open settings', 'View flow and quality checks']) expect(text).toContain(want)
    expect(text).not.toMatch(/split/i)
    const input = document.querySelector<HTMLInputElement>('[data-command-palette] input')!
    const set = (v: string) => act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, v)
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await set('canvas')
    const row = [...document.querySelectorAll<HTMLElement>('[role=option]')].find((o) => o.textContent?.includes('Toggle canvas'))!
    await act(async () => row.click())
    expect(toggleCanvas).toHaveBeenCalledOnce()
  })

  it('没匹配时写「没找到」与搜索词', async () => {
    useUiStore.setState({ invalidate: async () => {} })
    await mount(<CommandPalette onClose={() => {}} />)
    const input = document.querySelector<HTMLInputElement>('[data-command-palette] input')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'zzqx')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(document.querySelector('[data-command-palette]')!.textContent).toContain('Nothing found for “zzqx”')
  })
})
