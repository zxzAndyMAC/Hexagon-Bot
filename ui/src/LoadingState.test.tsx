// beautiful-ui 票 04：LoadingState Drive + WaitingReply 首 token 等待位。
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { LoadingState } from './components/LoadingState'
import { WaitingReply } from './components/Timeline'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  await act(async () => {})
  return { el, root }
}

beforeEach(() => { document.body.innerHTML = '' })

describe('LoadingState（beautiful-ui 票 04）', () => {
  it('Drive 像素格：9 格 chevron 延时表 + role=status + shimmer 标签', async () => {
    const { el, root } = await render(<LoadingState label="加载中" />)
    const status = el.querySelector('[role="status"]')!
    expect(status).not.toBeNull()
    const cells = el.querySelectorAll('.loading-cell')
    expect(cells.length).toBe(9)
    // chevron 延时表：(c+|r-1|)*90 → 角格 0ms，中列顶底 90/180…
    const delays = [...cells].map((c) => (c as HTMLElement).style.animationDelay)
    expect(delays[0]).toBe('90ms') // r=0,c=0 → (0+1)*90
    expect(delays[4]).toBe('90ms') // 中心 (1+0)*90
    expect(delays[8]).toBe('270ms') // r=2,c=2 → (2+1)*90
    expect(el.querySelector('.shimmer-text')!.textContent).toBe('加载中')
    expect(el.querySelector('.loading-elapsed')!.textContent).toContain('s')
    root.unmount()
  })

  it('计时从 startedAt 起算，不是挂载时刻', async () => {
    const { el, root } = await render(<LoadingState label="加载中" startedAt={Date.now() - 5000} />)
    expect(el.querySelector('.loading-elapsed')!.textContent).toContain('5.')
    root.unmount()
  })
})

describe('WaitingReply 首 token 等待位', () => {
  it('渲染等待气泡：角色名 + LoadingState', async () => {
    const { el, root } = await render(<WaitingReply role="后端开发" />)
    expect(el.querySelector('[data-waiting]')).not.toBeNull()
    expect(el.textContent).toContain('后端开发')
    // 标签走 timeline.working i18n——语言按测试环境解析，断言结构不断字面
    expect(el.querySelector('[data-waiting] .shimmer-text')!.textContent).toBeTruthy()
    expect(el.querySelectorAll('.loading-cell').length).toBe(9)
    root.unmount()
  })
})
