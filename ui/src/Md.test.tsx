import { describe, expect, it } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { Md } from './components/Md'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const DOC = [
  '# 标题一',
  '## 标题二',
  '### 标题三',
  '',
  '段落 **粗** *斜* ~~删~~ `inline` 结束。',
  '',
  '> 引用行',
  '',
  '---',
  '',
  '- 无序一',
  '  - 嵌套',
  '',
  '1. 有序一',
  '',
  '- [x] 已完成',
  '- [ ] 未完成',
  '',
  '| 列A | 列B |',
  '| --- | --- |',
  '| a1 | b1 |',
  '',
  '```ts',
  'const x: number = 1',
  '```',
  '',
  '```',
  'no lang block',
  '```',
  '',
  '[链接](https://example.com)',
].join('\n')

async function renderMd(md: string) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(<div className="msg-body"><Md>{md}</Md></div>) })
  return { el, root }
}

async function waitFor(el: Element, sel: string, ms = 15000) {
  const t0 = Date.now()
  while (Date.now() - t0 < ms) {
    const found = el.querySelector(sel)
    if (found) return found
    await new Promise((r) => setTimeout(r, 50))
  }
  return null
}

describe('Md markdown 渲染', () => {
  it('标题/段落/强调/删除线/行内代码/引用/分隔线/链接全部出现', async () => {
    const { el, root } = await renderMd(DOC)
    expect(el.querySelector('h1')?.textContent).toBe('标题一')
    expect(el.querySelector('h2')?.textContent).toBe('标题二')
    expect(el.querySelector('h3')?.textContent).toBe('标题三')
    expect(el.querySelector('strong')?.textContent).toBe('粗')
    expect(el.querySelector('em')?.textContent).toBe('斜')
    expect(el.querySelector('del')?.textContent).toBe('删')
    expect(el.querySelector('code')?.textContent).toBe('inline')
    expect(el.querySelector('blockquote')).toBeTruthy()
    expect(el.querySelector('hr')).toBeTruthy()
    const a = el.querySelector('a')
    expect(a?.getAttribute('href')).toBe('https://example.com')
    root.unmount()
  })

  it('GFM：表格带 thead/tbody，任务列表带 checkbox', async () => {
    const { el, root } = await renderMd(DOC)
    const table = el.querySelector('table')
    expect(table).toBeTruthy()
    expect(table!.querySelectorAll('thead th').length).toBe(2)
    expect(table!.querySelectorAll('tbody td').length).toBe(2)
    expect(el.querySelectorAll('ul li').length).toBeGreaterThanOrEqual(4)
    expect(el.querySelectorAll('ol li').length).toBe(1)
    const boxes = el.querySelectorAll('li input[type="checkbox"]')
    expect(boxes.length).toBe(2)
    expect((boxes[0] as HTMLInputElement).checked).toBe(true)
    expect((boxes[1] as HTMLInputElement).checked).toBe(false)
    root.unmount()
  })

  it('围栏代码块走高亮（Shiki），无语言块也不丢成内联', async () => {
    const { el, root } = await renderMd(DOC)
    // 两个围栏块：第一个 ts → .shiki-wrap；第二个无语言 → CodeBlock(text)
    const highlighted = await waitFor(el, '.shiki-wrap')
    expect(highlighted).toBeTruthy()
    // 渲染完成后应有两份代码块产物（高亮完成前是 .shiki-fallback）
    const blocks = el.querySelectorAll('.shiki-wrap, pre.shiki-fallback')
    expect(blocks.length).toBeGreaterThanOrEqual(2)
    root.unmount()
  }, 20000)
})
