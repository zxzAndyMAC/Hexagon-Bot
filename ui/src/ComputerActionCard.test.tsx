import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { PendingCard } from './components/PendingCards'
import { computerActionCall } from './computerActionCall'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import i18n from './i18n'

// 票 16B（control#action，components D8「电脑动作」）：computer_action 不再走通用权限卡。
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(async () => { await act(async () => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })
const card: PendingQuestion = { id: 'q-computer', kind: 'permission', agent_id: 'a-qa', state: 'queued',
  payload: { tool: 'computer_action', input: { op: 'click', app_id: 'System Settings', snapshot_id: 's1', x: 412, y: 288, element_id: 'screen-recording-toggle' }, safety_net: true, reason: 'safety net: desktop action with unverified external effects' } }

it('renders the computer action card: who/app/verb, kv rows, observe-first note and a once-only run button', async () => {
  await i18n.changeLanguage('zh-CN')
  useUiStore.setState({ team: [{ ...(useUiStore.getState().team[0] ?? {}), id: 'a-qa', role: 'QA' } as never] })
  const suggest = vi.spyOn(api, 'permissionShapeSuggestion')
  const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue(undefined)
  vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue(undefined)
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={card} top />) })
  expect(el.textContent).toContain('QA 想在「System Settings」里点击')
  expect(el.textContent).toContain('电脑动作 · 原生应用 · 每次确认')
  expect(el.textContent).toContain('click(412, 288)')
  expect(el.textContent).toContain('screen-recording-toggle')
  expect(el.textContent).toContain('结果不确定时，先观察再继续')
  expect(el.textContent).not.toContain('computer_action') // 不再露工具名与原始参数
  expect(el.querySelector('[aria-haspopup="menu"]')).toBeNull() // 安全网：没有范围菜单 / 记住
  expect(suggest).not.toHaveBeenCalled()
  const run = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('执行这一次'))!
  await act(async () => run.click())
  expect(answer).toHaveBeenCalledExactlyOnceWith('q-computer', true)
})

// ui-redesign gap D6：原型的截图缩略 + 「理由」行。理由来自 computer_action 入参 reason，缩略来自载荷
// screenshot（动作所依据那次观察的证据引用）；两者缺失时只是不出，卡照常可裁决（不挡、不自动批）。
it('shows the planned-on screenshot thumbnail and the reason row when the host sends them', async () => {
  await i18n.changeLanguage('zh-CN')
  useUiStore.setState({ team: [{ ...(useUiStore.getState().team[0] ?? {}), id: 'a-qa', role: 'QA' } as never] })
  const read = vi.spyOn(api, 'desktopScreenshot').mockResolvedValue({ data_url: 'data:image/png;base64,AA==' } as never)
  const shot = { screenshot_path: '/repo/.hexagon/computer-use/screenshots/0123.png', snapshot_id: 's1', title: 'Privacy', bundle_id: 'com.apple.systempreferences' }
  const q: PendingQuestion = { ...card, payload: { ...card.payload, input: { ...(card.payload.input as object), reason: '验证截图权限提示' }, screenshot: shot } }
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={q} top />) })
  const rows = [...el.querySelectorAll('.dock-kv-row')].map((r) => r.textContent)
  expect(rows).toContain('理由验证截图权限提示')
  const thumb = el.querySelector<HTMLButtonElement>('.computer-action-body .desktop-screenshot-thumbnail')!
  expect(thumb).not.toBeNull()
  expect(el.textContent).not.toContain('截图证据') // 卡内不重复说明行
  await act(async () => thumb.click())
  expect(read).toHaveBeenCalledExactlyOnceWith('0123.png', '/repo')
  expect(el.querySelector('dialog.desktop-screenshot-preview')).not.toBeNull()
  // 放大时裁决快捷键被拦：⌘↵ 不得批准
  const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue(undefined)
  await act(async () => { window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', metaKey: true, ctrlKey: true, bubbles: true })) })
  expect(answer).not.toHaveBeenCalled()
})

it('omits the thumbnail and reason row when the host has none, and the card still runs', async () => {
  await i18n.changeLanguage('zh-CN')
  useUiStore.setState({ team: [{ ...(useUiStore.getState().team[0] ?? {}), id: 'a-qa', role: 'QA' } as never] })
  const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue(undefined)
  vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue(undefined)
  const q: PendingQuestion = { ...card, payload: { ...card.payload, input: { ...(card.payload.input as object), reason: '   ' }, screenshot: null } }
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={q} top />) })
  expect(el.querySelector('.desktop-screenshot-thumbnail')).toBeNull()
  expect(el.textContent).not.toContain('理由')
  const run = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('执行这一次'))!
  await act(async () => run.click())
  expect(answer).toHaveBeenCalledExactlyOnceWith('q-computer', true)
})

it('formats the action call for every op', () => {
  expect(computerActionCall({ op: 'click', x: 1.4, y: 2, click_type: 'double' })).toBe('click(1, 2, double)')
  expect(computerActionCall({ op: 'type', text: 'hi' })).toBe('type("hi")')
  expect(computerActionCall({ op: 'key', chord: 'cmd,a' })).toBe('key(cmd,a)')
  expect(computerActionCall({ op: 'scroll', direction: 'down', amount: 3 })).toBe('scroll(down, 3)')
  expect(computerActionCall({ op: 'drag', x: 1, y: 2, to_x: 3, to_y: 4 })).toBe('drag(1, 2 → 3, 4)')
  expect(computerActionCall({ op: 'activate' })).toBe('activate()')
})
