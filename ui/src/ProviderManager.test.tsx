import { describe, expect, it, beforeEach, vi, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { ProviderManager } from './components/ProviderManager'
import { api } from './api'
import type { ProvidersView } from './gen/ProvidersView'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

const prov = (id: string, name: string, key_set: boolean, enabled: boolean) => ({
  id, name, kind: 'openai' as const, base_url: `https://${id}.example`, models: [], enabled, key_set,
})

const pv: ProvidersView = {
  providers: [
    prov('a', 'ReadyCo', true, true),
    prov('b', 'NoKeyCo', false, true),
    prov('c', 'OffCo', true, false),
  ],
  slots: { default: { provider_id: 'b', model: 'm1' } },
}

describe('ProviderManager 面板清扫（ui-audit-2 票 10）', () => {
  beforeEach(() => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(pv)
    vi.spyOn(api, 'presetRoles').mockResolvedValue([])
  })
  afterEach(() => vi.restoreAllMocks())

  it('左列状态点三态：已配 key=on / 缺 key=warn / 停用=off', async () => {
    const { el, root } = await render(<ProviderManager />)
    expect(el.querySelectorAll('.dot.on')).toHaveLength(1)
    expect(el.querySelectorAll('.dot.warn')).toHaveLength(1)
    expect(el.querySelectorAll('.dot.off')).toHaveLength(1)
    // 汇总行：default 绑到缺 key 的 NoKeyCo；decision 槽始终在列表里但未绑 → 1/2
    const text = el.textContent ?? ''
    expect(text).toContain('1/2')
    root.unmount()
  })

  it('检测按钮：缺 key 且未填 key 时禁用；已配 key 或本地端点可用', async () => {
    const { el, root } = await render(<ProviderManager />)
    const row = (name: string) =>
      [...el.querySelectorAll('div')].find((d) => d.textContent === name)!.closest('div')!
    // 点 NoKeyCo（缺 key，secret 空）→ 检测禁用
    await act(async () => row('NoKeyCo').dispatchEvent(new MouseEvent('click', { bubbles: true })))
    // settings-3col 票 04 加了搜索框——第一个 input 不再是名称框，按值找
    const nameInput = [...el.querySelectorAll('input')].find((i) => i.value === 'NoKeyCo')
    expect(nameInput).toBeTruthy() // 行点击真触发了 pick
    const checkBtn = () =>
      [...el.querySelectorAll('button')].find((b) => /check|检测/i.test(b.textContent ?? ''))!
    expect(checkBtn().disabled).toBe(true)
    // 点 ReadyCo（key_set=true）→ 检测可用
    await act(async () => row('ReadyCo').dispatchEvent(new MouseEvent('click', { bubbles: true })))
    expect(checkBtn().disabled).toBe(false)
    root.unmount()
  })
})
