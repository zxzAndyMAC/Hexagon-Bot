// beautiful-ui 票 08：CodeBlock 头部——语言标签 + 复制钮 + 已复制翻转。
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { CodeBlock, ReasoningMarkdown } from './components/Md'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  await act(async () => {})
  return { el, root }
}

beforeEach(() => {
  document.body.innerHTML = ''
})

it('长思考连续增量限频显示，结束立即保留完整 Markdown', async () => {
  vi.useFakeTimers()
  const onRender = vi.fn()
  const { el, root } = await render(<ReasoningMarkdown text="**开始**" live onRender={onRender} />)
  try {
    for (let i = 1; i <= 20; i++) {
      await act(async () => { root.render(<ReasoningMarkdown text={`**开始**\n\n尾部 ${i}`} live onRender={onRender} />) })
      await act(async () => { vi.advanceTimersByTime(10) })
    }
    expect(el.textContent).not.toContain('尾部')
    expect(onRender).toHaveBeenCalledTimes(1)
    await act(async () => { vi.advanceTimersByTime(50) })
    expect(el.textContent).toContain('尾部 20')
    expect(onRender).toHaveBeenCalledTimes(2)
    await act(async () => { root.render(<ReasoningMarkdown text="**开始**\n\n最终全部文本" live={false} onRender={onRender} />) })
    expect(el.textContent).toContain('最终全部文本')
    expect(el.querySelector('strong')?.textContent).toBe('开始')
  } finally {
    await act(async () => { root.unmount() })
    expect(vi.getTimerCount()).toBe(0)
    vi.useRealTimers()
  }
})

describe('CodeBlock 头部（beautiful-ui 票 08）', () => {
  it('渲染语言标签和复制钮；无 lang 回退 text', async () => {
    const { el, root } = await render(<CodeBlock code="const x = 1" lang="typescript" />)
    expect(el.querySelector('.code-head')).not.toBeNull()
    expect(el.querySelector('.code-lang')!.textContent).toBe('typescript')
    expect(el.querySelector('.code-copy')).not.toBeNull()
    root.unmount()

    const again = await render(<CodeBlock code="plain" />)
    expect(again.el.querySelector('.code-lang')!.textContent).toBe('text')
    again.root.unmount()
  })

  it('点复制写剪贴板并翻转已复制态', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    const { el, root } = await render(<CodeBlock code={'a\nb\n'} lang="json" />)
    const btn = el.querySelector('.code-copy')!
    await act(async () => { btn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(writeText).toHaveBeenCalledWith('a\nb') // 尾部换行剥掉
    expect(btn.textContent).toContain('Copied')
    expect(btn.querySelector('svg')).not.toBeNull() // check glyph
    root.unmount()
  })

  it('复制失败走 toast 不崩', async () => {
    const writeText = vi.fn().mockRejectedValue(new Error('denied'))
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    const { el, root } = await render(<CodeBlock code="x" />)
    await act(async () => {
      el.querySelector('.code-copy')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    // 未翻转成已复制
    expect(el.querySelector('.code-copy')!.textContent).not.toContain('Copied')
    root.unmount()
  })
})
