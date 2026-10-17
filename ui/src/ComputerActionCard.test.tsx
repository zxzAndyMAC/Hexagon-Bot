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
  payload: { tool: 'computer_action', input: { op: 'click', snapshot_id: 's1', x: 412, y: 288, element_id: 'screen-recording-toggle' }, safety_net: true, reason: 'safety net: desktop action with unverified external effects',
    app: { name: 'System Settings', title: null, bundle_id: 'com.apple.systempreferences' } } }

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

// ui-redesign gap D8：应用行 = 载荷 app（显示名 · 窗口标题，缺名回退 bundle id）；缩略上的目标圈只在
// 加载出的像素尺寸与证据记录一致、快照对得上时才画。D6 曾显示入参 app_id（不透明回执），现已不读。
it('names the app from the host payload and falls back to the bundle id, never the opaque app_id', async () => {
  await i18n.changeLanguage('zh-CN')
  useUiStore.setState({ team: [{ ...(useUiStore.getState().team[0] ?? {}), id: 'a-qa', role: 'QA' } as never] })
  const named: PendingQuestion = { ...card, payload: { ...card.payload, app: { name: '系统设置', title: '隐私与安全性', bundle_id: 'com.apple.systempreferences' } } }
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={named} top />) })
  expect(el.textContent).toContain('QA 想在「系统设置」里点击')
  expect([...el.querySelectorAll('.dock-kv-row')].map((r) => r.textContent)).toContain('应用系统设置 · 隐私与安全性')
  const bare: PendingQuestion = { ...card, payload: { ...card.payload, input: { op: 'activate', app_id: '6F1C-RECEIPT' }, app: { name: null, title: null, bundle_id: 'com.example.app' } } }
  await act(async () => { root.render(<PendingCard q={bare} top />) })
  expect(el.textContent).toContain('QA 想在「com.example.app」里切到前台')
  expect(el.textContent).not.toContain('6F1C-RECEIPT')
  const none: PendingQuestion = { ...card, payload: { ...card.payload, input: { op: 'activate', app_id: '6F1C-RECEIPT' }, app: null } }
  await act(async () => { root.render(<PendingCard q={none} top />) })
  expect(el.textContent).toContain('QA 想切到前台')
  expect(el.textContent).not.toContain('6F1C-RECEIPT')
  expect([...el.querySelectorAll('.dock-kv-row')].some((r) => r.textContent?.startsWith('应用'))).toBe(false)
})

async function renderWithLoadedThumb(shot: Record<string, unknown>, natural: { width: number; height: number }, input?: Record<string, unknown>) {
  vi.spyOn(api, 'desktopScreenshot').mockResolvedValue({ data_url: 'data:image/png;base64,AA==' } as never)
  const q: PendingQuestion = { ...card, payload: { ...card.payload, input: input ?? card.payload.input, screenshot: shot } }
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={q} top />) })
  const thumb = el.querySelector<HTMLButtonElement>('.computer-action-body .desktop-screenshot-thumbnail')!
  await act(async () => thumb.click())
  const img = thumb.querySelector('img')!
  Object.defineProperty(img, 'naturalWidth', { value: natural.width })
  Object.defineProperty(img, 'naturalHeight', { value: natural.height })
  await act(async () => { img.dispatchEvent(new Event('load')) })
  return thumb
}

it('draws the target ring at the mapped point once the recorded-size image has loaded', async () => {
  await i18n.changeLanguage('zh-CN')
  const shot = { screenshot_path: '/repo/.hexagon/computer-use/screenshots/0123.png', snapshot_id: 's1', image_width: 1600, image_height: 1000 }
  const thumb = await renderWithLoadedThumb(shot, { width: 1600, height: 1000 }, { op: 'click', snapshot_id: 's1', x: 400, y: 750 })
  const ring = thumb.querySelector<HTMLElement>('[data-testid="computer-target-ring"]')!
  expect(ring).not.toBeNull()
  expect(ring.style.left).toBe('25%')
  expect(ring.style.top).toBe('75%')
  expect(ring.getAttribute('aria-hidden')).toBe('true')
})

it('draws no ring when the loaded pixels, the snapshot or the recorded size cannot be trusted', async () => {
  await i18n.changeLanguage('zh-CN')
  const shot = { screenshot_path: '/repo/.hexagon/computer-use/screenshots/0123.png', snapshot_id: 's1', image_width: 1600, image_height: 1000 }
  const ringOf = (thumb: HTMLElement) => thumb.querySelector('[data-testid="computer-target-ring"]')
  // 加载出的图不是记录尺寸那张
  expect(ringOf(await renderWithLoadedThumb(shot, { width: 800, height: 500 }))).toBeNull()
  await act(async () => root.unmount()); vi.restoreAllMocks()
  // 缩略是另一快照
  expect(ringOf(await renderWithLoadedThumb({ ...shot, snapshot_id: 's0' }, { width: 1600, height: 1000 }))).toBeNull()
  await act(async () => root.unmount()); vi.restoreAllMocks()
  // D8 之前的证据行：没有像素尺寸
  expect(ringOf(await renderWithLoadedThumb({ ...shot, image_width: undefined, image_height: undefined }, { width: 1600, height: 1000 }))).toBeNull()
})

// review R4（reconciliation 偏差 12 剩余差异）：放大模态里也画圈（按图本身宽高比，不裁切）；drag 画起点 + 终点。
it('draws rings in the enlarged preview and both ends of a drag', async () => {
  await i18n.changeLanguage('zh-CN')
  const shot = { screenshot_path: '/repo/.hexagon/computer-use/screenshots/0123.png', snapshot_id: 's1', image_width: 1000, image_height: 1000 }
  // 方图：缩略 16:10 裁掉上下，终点 y=950 落在裁掉的部分，只剩起点；放大模态不裁切，两圈都在
  await renderWithLoadedThumb(shot, { width: 1000, height: 1000 }, { op: 'drag', snapshot_id: 's1', x: 250, y: 500, to_x: 750, to_y: 950 })
  const thumb = el.querySelector<HTMLElement>('.computer-action-body .desktop-screenshot-thumbnail')!
  const rings = (root: Element) => [...root.querySelectorAll<HTMLElement>('[data-testid="computer-target-ring"]')]
    .map((r) => [r.dataset.ring, r.style.left, r.style.top])
  expect(rings(thumb)).toEqual([['start', '25%', '50%']])
  const preview = el.querySelector('dialog.desktop-screenshot-preview')!
  expect(rings(preview)).toEqual([['start', '25%', '50%'], ['end', '75%', '95%']])
})

it('draws no drag rings when the end point is unproven', async () => {
  await i18n.changeLanguage('zh-CN')
  const shot = { screenshot_path: '/repo/.hexagon/computer-use/screenshots/0123.png', snapshot_id: 's1', image_width: 1600, image_height: 1000 }
  await renderWithLoadedThumb(shot, { width: 1600, height: 1000 }, { op: 'drag', snapshot_id: 's1', x: 400, y: 500, to_x: 1600, to_y: 500 })
  expect(el.querySelectorAll('[data-testid="computer-target-ring"]')).toHaveLength(0)
})

it('formats the action call for every op', () => {
  expect(computerActionCall({ op: 'click', x: 1.4, y: 2, click_type: 'double' })).toBe('click(1, 2, double)')
  expect(computerActionCall({ op: 'type', text: 'hi' })).toBe('type("hi")')
  expect(computerActionCall({ op: 'key', chord: 'cmd,a' })).toBe('key(cmd,a)')
  expect(computerActionCall({ op: 'scroll', direction: 'down', amount: 3 })).toBe('scroll(down, 3)')
  expect(computerActionCall({ op: 'drag', x: 1, y: 2, to_x: 3, to_y: 4 })).toBe('drag(1, 2 → 3, 4)')
  expect(computerActionCall({ op: 'activate' })).toBe('activate()')
})
