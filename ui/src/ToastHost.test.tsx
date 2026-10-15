import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import i18n from './i18n'
import { useUiStore } from './store'
import { ToastHost } from './components/ToastHost'
import { toastError, reasonCode } from './toast'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

// ui-redesign 票 13（components A18，feedback#toast）：Toast 改 Sonner 形制——
// 胶囊 + 前置图标 + 至多一个文字动作；失败 role=alert；约 3.5s 消失、悬停暂停；不抢焦点；F6 跳到通知区。

beforeAll(async () => { await i18n.changeLanguage('zh-CN') })

let root: Root
let el: HTMLElement
beforeEach(async () => {
  useUiStore.setState({ toasts: [] })
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => root.render(<ToastHost />))
})
afterEach(async () => {
  await act(async () => root.unmount())
  el.remove()
  vi.useRealTimers()
})

const pills = () => [...document.querySelectorAll<HTMLElement>('[data-slot="toast"]')]
// Sonner 新条在前（底部堆叠），按文案取，不按 DOM 次序
const pill = (text: string) => pills().find((p) => p.textContent?.includes(text))!
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)) })

describe('ToastHost（A18）', () => {
  it('失败 toast 用 role=alert，成功用 role=status，且不抢焦点', async () => {
    const before = document.activeElement
    await act(async () => {
      useUiStore.getState().pushToast('已导出 412 条轨迹', 'ok')
      useUiStore.getState().pushToast('切换项目失败：目录不存在', 'err')
    })
    await flush()
    const ok = pill('已导出'), err = pill('切换项目失败')
    expect(ok.getAttribute('role')).toBe('status')
    expect(err.getAttribute('role')).toBe('alert')
    expect(err.dataset.tone).toBe('err')
    expect(document.activeElement).toBe(before)
  })

  it('原因码随失败文案出现（D8：worktree_binding_drift）', async () => {
    const drift = { code: 'worktree_binding_drift', message: 'worktree HEAD moved' }
    expect(reasonCode(drift)).toBe('worktree_binding_drift')
    expect(reasonCode({ code: 'internal', message: 'x' })).toBeUndefined()
    await act(async () => { toastError(drift) })
    await flush()
    const p = pill(i18n.t('errors.worktree_binding_drift'))
    expect(p.getAttribute('role')).toBe('alert')
    expect(p.textContent).toContain('（worktree_binding_drift）')
  })

  it('文字动作：点击执行并收起；F6 把焦点送到动作上', async () => {
    const run = vi.fn()
    await act(async () => { useUiStore.getState().pushToast('切换项目失败', 'err', { action: { label: '从最近项目移除', run } }) })
    await flush()
    const action = document.querySelector<HTMLButtonElement>('[data-toast-action]')!
    expect(action.textContent).toBe('从最近项目移除')
    await act(async () => { window.dispatchEvent(new KeyboardEvent('keydown', { key: 'F6', bubbles: true })) })
    expect(document.activeElement).toBe(action)
    await act(async () => { action.click() })
    await flush()
    expect(run).toHaveBeenCalledTimes(1)
  })

  it('约 3.5s 后出队；store 不再自己计时', async () => {
    vi.useFakeTimers()
    await act(async () => { useUiStore.getState().pushToast('已撤销规则', 'info', { icon: 'undo' }) })
    await act(async () => { vi.advanceTimersByTime(100) })
    expect(useUiStore.getState().toasts).toHaveLength(1)
    await act(async () => { vi.advanceTimersByTime(5000) })
    expect(useUiStore.getState().toasts).toHaveLength(0)
  })
})
