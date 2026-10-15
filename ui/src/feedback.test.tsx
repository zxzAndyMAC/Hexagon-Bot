import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { useUiStore } from './store'
import { switchProject, stayInProject, useSwitchWait, SWITCH_POLL_MS } from './projectSwitch'
import { SwitchWaitBanner } from './components/SwitchWaitBanner'
import { ConfirmHost } from './components/ConfirmDialog'
import type { DesktopStatus } from './gen/DesktopStatus'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

// ui-redesign 票 13（反馈层，interaction §13）：确认层、切换项目阻塞横条、原生 confirm 守门。

beforeAll(async () => { await i18n.changeLanguage('zh-CN') })

let root: Root | undefined
let el: HTMLElement | undefined
async function render(node: React.ReactNode) {
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => root!.render(node))
}
afterEach(async () => {
  if (root) await act(async () => root!.unmount())
  el?.remove()
  root = undefined
  vi.useRealTimers()
  vi.restoreAllMocks()
  useUiStore.setState({ confirmReq: null })
  useSwitchWait.setState({ wait: null })
})

const status = (busy: boolean) => ({ busy } as DesktopStatus)

describe('原生 confirm() 守门（interaction §13 确认层）', () => {
  it('源码里没有 window.confirm / 裸 confirm() 调用', () => {
    // Vite 原样读入全部源码（不含测试与生成物）——新增原生 confirm 即红
    const sources = import.meta.glob(['./**/*.{ts,tsx}', '!./**/*.test.{ts,tsx}', '!./gen/**'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>
    expect(Object.keys(sources).length).toBeGreaterThan(50)
    const offenders = Object.entries(sources)
      .filter(([, src]) => /(window|globalThis)\.confirm\s*\(|(^|[^\w.])confirm\s*\(\s*['"`]/m.test(src))
      .map(([file]) => file)
    expect(offenders).toEqual([])
  })
})

describe('确认层走共享 AlertDialog（A17）', () => {
  it('破坏性确认首焦「取消」；安全类（全员休眠）首焦主按钮并带 ↵', async () => {
    await render(<ConfirmHost />)
    await act(async () => { useUiStore.getState().askConfirm({ title: '移除 MCP 服务 figma？', danger: true, run: () => {} }) })
    const dialog = document.querySelector('[role="alertdialog"]')!
    expect(dialog.querySelector('[data-slot="alert-dialog-title"]')?.textContent).toBe('移除 MCP 服务 figma？')
    expect(document.activeElement?.getAttribute('data-confirm')).toBe('cancel')
    expect(dialog.querySelector('[data-confirm="confirm"]')?.getAttribute('data-variant')).toBe('danger')
    await act(async () => { useUiStore.getState().clearConfirm() })

    await act(async () => { useUiStore.getState().askConfirm({ title: '让全员休眠？', run: () => {} }) })
    const confirm = document.querySelector('[role="alertdialog"] [data-confirm="confirm"]')!
    expect(document.activeElement).toBe(confirm)
    expect(confirm.getAttribute('data-variant')).toBe('primary')
    expect(confirm.textContent).toContain('↵')
  })

  it('确认失败：走失败 toast（role=alert 由 ToastHost 渲染），确认层收起', async () => {
    useUiStore.setState({ toasts: [] })
    await render(<ConfirmHost />)
    await act(async () => { useUiStore.getState().askConfirm({ title: '移除？', danger: true, skipRefresh: true, run: () => Promise.reject({ code: 'worktree_binding_drift', message: 'x' }) }) })
    await act(async () => { document.querySelector<HTMLButtonElement>('[data-confirm="confirm"]')!.click() })
    expect(useUiStore.getState().toasts.at(-1)).toMatchObject({ tone: 'err', code: 'worktree_binding_drift' })
    expect(document.querySelector('[role="alertdialog"]')).toBeNull()
  })
})

describe('切换项目阻塞横条（feedback#errors）', () => {
  it('电脑动作未排空：先出横条，排空后才 open_recent', async () => {
    vi.useFakeTimers()
    useUiStore.setState({ projectName: 'todo-pwa' })
    const desk = vi.spyOn(api, 'desktopStatus').mockResolvedValueOnce(status(true)).mockResolvedValueOnce(status(true)).mockResolvedValue(status(false))
    const open = vi.spyOn(api, 'openRecent').mockResolvedValue(undefined as never)
    const refresh = vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue(undefined)
    await render(<SwitchWaitBanner />)
    let done: Promise<boolean> | undefined
    await act(async () => { done = switchProject('/p/other') })
    expect(el!.textContent).toContain('切换项目需要等待')
    expect(el!.textContent).toContain('todo-pwa')
    expect(open).not.toHaveBeenCalled()
    await act(async () => { await vi.advanceTimersByTimeAsync(SWITCH_POLL_MS) })
    expect(open).not.toHaveBeenCalled()
    await act(async () => { await vi.advanceTimersByTimeAsync(SWITCH_POLL_MS) })
    expect(await done).toBe(true)
    expect(open).toHaveBeenCalledWith('/p/other')
    expect(refresh).toHaveBeenCalled()
    expect(desk).toHaveBeenCalledTimes(3)
    expect(el!.querySelector('[data-switch-wait]')).toBeNull()
  })

  it('「留在此项目」撤回切换，不调 open_recent', async () => {
    vi.useFakeTimers()
    vi.spyOn(api, 'desktopStatus').mockResolvedValue(status(true))
    const open = vi.spyOn(api, 'openRecent').mockResolvedValue(undefined as never)
    await render(<SwitchWaitBanner />)
    let done: Promise<boolean> | undefined
    await act(async () => { done = switchProject('/p/other') })
    const stay = [...el!.querySelectorAll('button')].find((b) => b.textContent === '留在此项目')!
    await act(async () => { stay.click() })
    await act(async () => { await vi.advanceTimersByTimeAsync(SWITCH_POLL_MS) })
    expect(await done).toBe(false)
    expect(open).not.toHaveBeenCalled()
    expect(el!.querySelector('[data-switch-wait]')).toBeNull()
    stayInProject()
  })

  it('没有电脑动作时直接切换，不出横条；失败走 toast', async () => {
    useUiStore.setState({ toasts: [] })
    vi.spyOn(api, 'desktopStatus').mockRejectedValue(new Error('no desktop'))
    vi.spyOn(api, 'openRecent').mockRejectedValue({ code: 'x', message: '目录不存在' })
    expect(await switchProject('/gone')).toBe(false)
    expect(useSwitchWait.getState().wait).toBeNull()
    expect(useUiStore.getState().toasts.at(-1)).toMatchObject({ text: '目录不存在', tone: 'err' })
  })
})
