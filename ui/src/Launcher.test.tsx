import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
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
