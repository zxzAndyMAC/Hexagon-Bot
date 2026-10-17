// review S5（2026-10-17）：useBusyAction / useCopyFlash 从 6 处 + 3 处同形代码抽出，
// 钉住抽出前的行为——忙态起落、失败 toast、「已复制」停留 COPY_FLASH_MS 后复位、再次复制重新计时。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { useUiStore } from './store'
import { useBusyAction } from './useBusyAction'
import { COPY_FLASH_MS, useCopyFlash } from './useCopyFlash'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const initial = useUiStore.getState()
const el = document.createElement('div')
let root: ReturnType<typeof createRoot> | undefined

function mount<T>(hook: () => T): { current: T } {
  const out = {} as { current: T }
  function Probe() { out.current = hook(); return null }
  root = createRoot(el)
  act(() => root!.render(<Probe />))
  return out
}

beforeEach(() => { document.body.appendChild(el) })
afterEach(async () => {
  if (root) await act(async () => root!.unmount())
  root = undefined
  el.remove()
  vi.useRealTimers()
  vi.restoreAllMocks()
  useUiStore.setState(initial, true)
})

it('useBusyAction: busy while running, resets after success', async () => {
  const h = mount(useBusyAction)
  let release!: () => void
  let done!: Promise<void>
  act(() => { done = h.current.run(() => new Promise<void>((r) => { release = r })) })
  expect(h.current.busy).toBe(true)
  await act(async () => { release(); await done })
  expect(h.current.busy).toBe(false)
  expect(useUiStore.getState().toasts).toEqual([])
})

it('useBusyAction: failure toasts the error text and resets busy', async () => {
  const h = mount(useBusyAction)
  await act(async () => { await h.current.run(async () => { throw new Error('boom') }) })
  expect(h.current.busy).toBe(false)
  expect(useUiStore.getState().toasts.map((t) => [t.text, t.tone])).toEqual([['boom', 'err']])
})

it('useCopyFlash: writes text, flashes for COPY_FLASH_MS, re-copy restarts the timer', async () => {
  vi.useFakeTimers()
  const writeText = vi.fn().mockResolvedValue(undefined)
  vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
  const h = mount(useCopyFlash)
  await act(async () => { await h.current.copy('hello') })
  expect(writeText).toHaveBeenCalledWith('hello')
  expect(h.current.copied).toBe(true)
  act(() => { vi.advanceTimersByTime(COPY_FLASH_MS - 100) })
  await act(async () => { await h.current.copy('again') })
  act(() => { vi.advanceTimersByTime(COPY_FLASH_MS - 100) })
  expect(h.current.copied).toBe(true)
  act(() => { vi.advanceTimersByTime(100) })
  expect(h.current.copied).toBe(false)
  vi.unstubAllGlobals()
})

it('useCopyFlash: clipboard failure toasts and never flashes', async () => {
  vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: vi.fn().mockRejectedValue(new Error('denied')) } })
  const h = mount(useCopyFlash)
  await act(async () => { await h.current.copy('x') })
  expect(h.current.copied).toBe(false)
  expect(useUiStore.getState().toasts.map((t) => t.tone)).toEqual(['err'])
  vi.unstubAllGlobals()
})
