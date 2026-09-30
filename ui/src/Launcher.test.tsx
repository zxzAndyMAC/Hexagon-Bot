import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { Launcher } from './components/Launcher'
import type { RecentProject } from './gen/RecentProject'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
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
    expect(dead.querySelector('.chip.err')?.textContent).toContain('目录已删除')
    await act(async () => { dead.click() })
    expect(open).not.toHaveBeenCalled()

    const ok = recentRow(el!, 'ok-proj')
    expect(ok.getAttribute('data-missing')).toBeNull()
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

    const btn = recentRow(el!, 'dead-proj').querySelector('button[aria-label]') as HTMLButtonElement
    await act(async () => { btn.click() })

    expect(rm).toHaveBeenCalledWith('/gone/dead-proj')
    expect(open).not.toHaveBeenCalled()
    expect(el!.textContent).not.toContain('dead-proj')
    expect(el!.textContent).toContain('ok-proj')
  })
})

// owner 2026-09-30：向导期间右上角设置钮恒显——按钮从启动页容器提升到
// Launcher 顶层（fixed z70 > 向导 z60），设置层 z80 盖住向导但不替换。
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

  const gear = (el: HTMLElement) =>
    [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === '设置') as HTMLButtonElement

  it('进向导后设置钮仍在；点击开设置层，返回后向导原样', async () => {
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))
    expect(gear(el!)).toBeTruthy()

    const newBtn = [...el!.querySelectorAll('button')].find((b) => b.textContent?.trim() === '新建项目')!
    await act(async () => { newBtn.click() })
    expect(el!.textContent).toContain('项目向导')
    expect(gear(el!)).toBeTruthy()

    await act(async () => { gear(el!).click() })
    expect(el!.textContent).toContain('返回工作台')
    expect(el!.textContent).toContain('项目向导') // 盖而不换：向导保持挂载

    const back = [...el!.querySelectorAll('button')].find((b) => b.textContent?.includes('返回工作台'))!
    await act(async () => { back.click() })
    expect(el!.textContent).not.toContain('返回工作台')
    expect(el!.textContent).toContain('项目向导')
    expect(gear(el!)).toBeTruthy()
  })

  it('hexagon:open-settings 广播在向导期收成 toggle（与 ⌘, 同语义）', async () => {
    ;({ el, root } = await render(<Launcher onOpen={() => {}} />))
    const newBtn = [...el!.querySelectorAll('button')].find((b) => b.textContent?.trim() === '新建项目')!
    await act(async () => { newBtn.click() })
    expect(el!.textContent).toContain('项目向导')

    await act(async () => {
      window.dispatchEvent(new CustomEvent('hexagon:open-settings'))
    })
    expect(el!.textContent).toContain('返回工作台')
    await act(async () => {
      window.dispatchEvent(new CustomEvent('hexagon:open-settings'))
    })
    expect(el!.textContent).not.toContain('返回工作台')
    expect(el!.textContent).toContain('项目向导')
  })
})
