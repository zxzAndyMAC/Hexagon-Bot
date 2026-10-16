// 票 16a 回归：浮条打开期间把预览 / 代码视图里的选区钉成 accent-tint 高亮（components D14、canvas#file `.hl`）。
// 原先只靠原生 ::selection，焦点进「描述修改…」输入框后选区消失。jsdom 没有 CSS Custom Highlight API，这里桩一份。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { SelectionBar, SELBAR_HIGHLIGHT } from './SelectionBar'
import { TooltipProvider } from '@/components/ui/tooltip'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const registry = new Map<string, unknown>()
class FakeHighlight { ranges: Range[]; constructor(...r: Range[]) { this.ranges = r } }
// 测试环境（happy-dom）的 CSS 是每次取都新建的只读 getter，不能就地挂属性：整体 stubGlobal 换掉
beforeEach(() => {
  vi.stubGlobal('CSS', { highlights: registry })
  vi.stubGlobal('Highlight', FakeHighlight)
})
afterEach(() => {
  vi.unstubAllGlobals()
  registry.clear()
  document.body.innerHTML = ''
})

function selectIn(selector: string, html: string) {
  const host = document.createElement('div')
  host.innerHTML = html
  document.body.appendChild(host)
  const target = host.querySelector(selector)!
  const range = document.createRange()
  range.selectNodeContents(target)
  const sel = window.getSelection()!
  sel.removeAllRanges()
  sel.addRange(range)
  return target.textContent ?? ''
}

async function mountBar(text: string) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  const anchor = { sel: { path: 'README.md', text }, x: 10, y: 60, bottom: 80 }
  await act(async () => root.render(<TooltipProvider><SelectionBar anchor={anchor} onClose={() => {}} /></TooltipProvider>))
  return root
}

it('预览里的选区在浮条打开期间被钉成高亮，浮条关闭即撤', async () => {
  const text = selectIn('li', '<div data-file-preview><ul><li>列表项一</li></ul></div>')
  const root = await mountBar(text)
  const h = registry.get(SELBAR_HIGHLIGHT) as FakeHighlight | undefined
  expect(h).toBeDefined()
  expect(h!.ranges[0].toString()).toBe('列表项一')
  // 焦点进输入框（原生选区丢失）后高亮仍在
  await act(async () => { (document.querySelector('[data-selbar] input') as HTMLInputElement).focus() })
  expect(registry.has(SELBAR_HIGHLIGHT)).toBe(true)
  await act(async () => root.render(<TooltipProvider><SelectionBar anchor={null} onClose={() => {}} /></TooltipProvider>))
  expect(registry.has(SELBAR_HIGHLIGHT)).toBe(false)
  root.unmount()
})

it('不在预览 / 代码视图里的选区不钉', async () => {
  const text = selectIn('p', '<div><p>别处的字</p></div>')
  const root = await mountBar(text)
  expect(registry.has(SELBAR_HIGHLIGHT)).toBe(false)
  root.unmount()
})
