import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { Launcher } from './components/Launcher'
import { TooltipProvider } from './components/ui/tooltip'
import type { RecentProject } from './gen/RecentProject'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

// ui-redesign 票 10：启动页按 entry#launcher / components E11 重做。设置钮换成 A2 图标按钮
// （可读名「设置」，tooltip 带 ⌘, 绑定），「新建项目」换成「空白新建」，新增一句话开局。
// 已删目录行、移除钮、设置层盖住向导的既有行为不变。

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(<TooltipProvider>{node}</TooltipProvider>) })
  return { el, root }
}

const RECENTS: RecentProject[] = [
  { dir: '/tmp/ok-proj', name: 'ok-proj', mode: 'pack', opened_at: 1, exists: true },
  { dir: '/gone/dead-proj', name: 'dead-proj', mode: 'fastpath', opened_at: 0, exists: false },
]

function recentRow(el: HTMLElement, name: string) {
  return [...el.querySelectorAll('.recent-row')].find((n) => n.textContent?.includes(name)) as HTMLElement
}

describe('启动页最近项目：已删目录容错（owner 反馈）', () => {
  let root: Root | undefined
  let el: HTMLElement | undefined

  afterEach(() => {
    act(() => { root?.unmount() })
    el?.remove()
    root = undefined
    el = undefined
    vi.restoreAllMocks()
  })

  it('已删目录红标 + missing chip，点击不发起 openRecent；存在的照常打开', async () => {
    vi.spyOn(api, 'recentProjects').mockResolvedValue(RECENTS)
    const open = vi.spyOn(api, 'openRecent').mockResolvedValue(undefined)
    const onOpen = vi.fn()
    ;({ el, root } = await render(<Launcher onOpen={onOpen} />))

    const dead = recentRow(el!, 'dead-proj')
    expect(dead.getAttribute('data-missing')).toBe('1')
    expect(dead.querySelector('[data-slot=badge][data-variant=err]')?.textContent).toContain('目录已删除')
    await act(async () => { dead.click() })
    expect(open).not.toHaveBeenCalled()

    const ok = recentRow(el!, 'ok-proj')
    expect(ok.getAttribute('data-missing')).toBeNull()
    expect(ok.textContent).toContain('流程包')
    await act(async () => { ok.click() })
    expect(open).toHaveBeenCalledWith('/tmp/ok-proj')
    expect(onOpen).toHaveBeenCalled()
  })

  it('每行 × 钮走 remove_recent 并重拉列表，不触发行打开', async () => {
    vi.spyOn(api, 'recentProjects')
      .mockResolvedValueOnce(RECENTS)
      .mockResolvedValue([RECENTS[0]])
    const rm = vi.spyOn(api, 'removeRecent').mockResolvedValue(undefined)
    const open = vi.spyOn(api, 'openRecent').mockResolvedValue(undefined)
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))

    const btn = recentRow(el!, 'dead-proj').querySelector('button[aria-label="从列表移除"]') as HTMLButtonElement
    await act(async () => { btn.click() })

    expect(rm).toHaveBeenCalledWith('/gone/dead-proj')
    expect(open).not.toHaveBeenCalled()
    expect(el!.textContent).not.toContain('dead-proj')
    expect(el!.textContent).toContain('ok-proj')
  })

  it('打开失败走 toast，不进工作台', async () => {
    vi.spyOn(api, 'recentProjects').mockResolvedValue([RECENTS[0]])
    vi.spyOn(api, 'openRecent').mockRejectedValue({ code: 'x', message: '打不开' })
    const { useUiStore } = await import('./store')
    const push = vi.spyOn(useUiStore.getState(), 'pushToast')
    const onOpen = vi.fn()
    ;({ el, root } = await render(<Launcher onOpen={onOpen} />))
    await act(async () => { recentRow(el!, 'ok-proj').click() })
    // 票 13（feedback#toast / A18）：失败 toast 带「从最近项目移除」动作（原断言只有文案与 tone，ADR 0051 规范补齐）。
    expect(push).toHaveBeenCalledWith('打不开', 'err', expect.objectContaining({ action: expect.objectContaining({ label: '从最近项目移除' }) }))
    expect(onOpen).not.toHaveBeenCalled()
    const rm = vi.spyOn(api, 'removeRecent').mockResolvedValue(undefined as never)
    await act(async () => { await push.mock.calls[0][2]!.action!.run() })
    expect(rm).toHaveBeenCalledWith('/tmp/ok-proj')
  })

  it('没有最近项目时显示空态', async () => {
    vi.spyOn(api, 'recentProjects').mockResolvedValue([])
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))
    await act(async () => { await Promise.resolve() })
    expect(el!.textContent).toContain('还没有项目')
    expect(el!.querySelector('.recent-row')).toBeNull()
  })

  // 2026-10-26 负责人反馈：启动页 / 向导拖不动窗口（裸 drag-region 只认点在元素本身，内容列铺满底板）；
  // 最近项目多了整页变高滚动。jsdom 不排版，这里钉结构：顶部独立拖动条、列表自身是滚动容器。
  it('顶部有独立拖动条（向导期仍在）；最近项目列表自己滚动', async () => {
    vi.spyOn(api, 'recentProjects').mockResolvedValue(RECENTS)
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))
    await act(async () => { await Promise.resolve() })
    const drag = el!.querySelector<HTMLElement>('[data-launch-drag]')!
    expect(drag.hasAttribute('data-tauri-drag-region')).toBe(true)
    expect(drag.children).toHaveLength(0)
    expect(el!.querySelector('[data-launch-recents]')!.className).toContain('overflow-y-auto')
    await act(async () => { el!.querySelector<HTMLElement>('[data-blank-new]')!.click() })
    expect(el!.querySelector('[data-wizard]')).not.toBeNull()
    expect(el!.querySelector('[data-launch-drag]')).not.toBeNull()
  })
})

describe('一句话开局带入向导（票 10，interaction §11）', () => {
  let root: Root | undefined
  let el: HTMLElement | undefined

  beforeEach(() => {
    localStorage.removeItem('hexagon.wizard')
    vi.spyOn(api, 'recentProjects').mockResolvedValue([])
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([])
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
  })
  afterEach(() => {
    act(() => { root?.unmount() })
    el?.remove()
    root = undefined
    el = undefined
    localStorage.removeItem('hexagon.wizard')
    vi.restoreAllMocks()
  })

  function type(box: HTMLTextAreaElement, value: string) {
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!
    setter.call(box, value)
    box.dispatchEvent(new Event('input', { bubbles: true }))
  }

  it('↵ 开始：一句话写进向导草稿的 brief，向导仍从模型服务商开始', async () => {
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))
    const box = el!.querySelector('[data-launch-prompt]') as HTMLTextAreaElement
    expect(document.activeElement).toBe(box)
    const send = el!.querySelector('[data-launch-start]')!
    expect(send.getAttribute('aria-disabled')).toBe('true')
    await act(async () => { send.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(el!.querySelector('[data-wizard]')).toBeNull()
    await act(async () => { type(box, '离线待办 PWA') })
    // ⇧↵ 是换行，不开始
    await act(async () => { box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, bubbles: true })) })
    expect(el!.querySelector('[data-wizard]')).toBeNull()
    await act(async () => { box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })) })
    expect(el!.querySelector('[data-wizard]')).toBeTruthy()
    expect(el!.querySelector('[aria-current="step"]')!.getAttribute('data-step')).toBe('providers')
    expect(JSON.parse(localStorage.getItem('hexagon.wizard')!).brief).toBe('离线待办 PWA')
  })

  it('空白新建不带一句话；向导第一步「取消」回启动页、草稿保留', async () => {
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))
    await act(async () => { (el!.querySelector('[data-blank-new]') as HTMLButtonElement).click() })
    expect(el!.querySelector('[data-wizard]')).toBeTruthy()
    expect(JSON.parse(localStorage.getItem('hexagon.wizard')!).brief).toBe('')
    const cancel = [...el!.querySelectorAll('button')].find((b) => b.textContent?.trim() === '取消')!
    await act(async () => { cancel.click() })
    expect(el!.querySelector('[data-wizard]')).toBeNull()
    expect(localStorage.getItem('hexagon.wizard')).not.toBeNull()
  })
})

// owner 2026-09-30：向导期间右上角设置钮恒显——按钮在 Launcher 顶层
// （fixed z70 > 向导 z60），设置层 z80 盖住向导但不替换。
describe('向导期间设置钮恒显', () => {
  let root: Root | undefined
  let el: HTMLElement | undefined

  beforeEach(() => {
    localStorage.removeItem('hexagon.wizard')
    vi.spyOn(api, 'recentProjects').mockResolvedValue([])
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([])
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    vi.spyOn(api, 'logEnabled').mockResolvedValue(false)
  })
  afterEach(() => {
    act(() => { root?.unmount() })
    el?.remove()
    root = undefined
    el = undefined
    vi.restoreAllMocks()
  })

  // 票 10：设置钮由文字按钮改为 A2 图标按钮，按可读名找
  const gear = (el: HTMLElement) => el.querySelector('button[aria-label="设置"]') as HTMLButtonElement
  const wizardShown = (el: HTMLElement) => !!el.querySelector('[data-wizard]')

  it('进向导后设置钮仍在；点击开设置层，返回后向导原样', async () => {
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))
    expect(gear(el!)).toBeTruthy()

    await act(async () => { (el!.querySelector('[data-blank-new]') as HTMLButtonElement).click() })
    expect(wizardShown(el!)).toBe(true)
    expect(gear(el!)).toBeTruthy()

    await act(async () => { gear(el!).click() })
    expect(el!.textContent).toContain('返回工作台')
    expect(wizardShown(el!)).toBe(true) // 盖而不换：向导保持挂载

    const back = [...el!.querySelectorAll('button')].find((b) => b.textContent?.includes('返回工作台'))!
    await act(async () => { back.click() })
    expect(el!.textContent).not.toContain('返回工作台')
    expect(wizardShown(el!)).toBe(true)
    expect(gear(el!)).toBeTruthy()
  })

  it('hexagon:open-settings 广播在向导期收成 toggle（与 ⌘, 同语义）', async () => {
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))
    await act(async () => { (el!.querySelector('[data-blank-new]') as HTMLButtonElement).click() })
    expect(wizardShown(el!)).toBe(true)

    await act(async () => {
      window.dispatchEvent(new CustomEvent('hexagon:open-settings'))
    })
    expect(el!.textContent).toContain('返回工作台')
    await act(async () => {
      window.dispatchEvent(new CustomEvent('hexagon:open-settings'))
    })
    expect(el!.textContent).not.toContain('返回工作台')
    expect(wizardShown(el!)).toBe(true)
  })
})
