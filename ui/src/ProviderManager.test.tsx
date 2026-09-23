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
    // 汇总行：default 已绑但缺钥匙。列表里还有 decision、三个起草槽和 jev，共 6 个。
    const text = el.textContent ?? ''
    expect(text).toContain('1/6')
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

// context-window 票 02：窗口/输出上限元数据在设置页可见可改
describe('ProviderManager 模型窗口元数据（context-window 票 02）', () => {
  const withModels: ProvidersView = {
    providers: [
      {
        ...prov('a', 'ReadyCo', true, true),
        models: [
          { id: 'deepseek-chat', name: null, group: 'deepseek', caps: ['tools'], context_window: 65536, max_output: 8192 },
          { id: 'mystery-x', name: null, group: null, caps: [], context_window: null, max_output: null },
        ],
      },
    ],
    slots: {},
  }

  beforeEach(() => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(withModels)
    vi.spyOn(api, 'presetRoles').mockResolvedValue([])
    vi.spyOn(api, 'saveProvider').mockResolvedValue(undefined)
  })
  afterEach(() => vi.restoreAllMocks())

  it('模型行：已知窗口显示 k 值，未识别挂未知警告标', async () => {
    const { el, root } = await render(<ProviderManager />)
    const row = (name: string) =>
      [...el.querySelectorAll('div')].find((d) => d.textContent === name)!.closest('div')!
    await act(async () => row('ReadyCo').dispatchEvent(new MouseEvent('click', { bubbles: true })))
    expect(el.textContent).toContain('64k')
    // 未识别模型 → 回落 120k 的警告标要看得见
    expect(el.textContent).toMatch(/window\?|窗口未知|視窗未知|ウィンドウ不明|inconnue|desc\./)
    root.unmount()
  })

  it('编辑面板写入窗口/输出上限，保存时随模型条目上送', async () => {
    const { el, root } = await render(<ProviderManager />)
    const row = (name: string) =>
      [...el.querySelectorAll('div')].find((d) => d.textContent === name)!.closest('div')!
    await act(async () => row('ReadyCo').dispatchEvent(new MouseEvent('click', { bubbles: true })))
    // mystery-x 行的编辑钮（行内第一枚按钮）。含此文本的 .panel 有外层
    // 详情卡和行本体两级——取最内层（document 序最后一个）。
    const mrow = [...el.querySelectorAll('div.panel')].filter((d) => d.textContent?.includes('mystery-x')).at(-1)!
    const editBtn = mrow.querySelector('button')!
    await act(async () => editBtn.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    const nums = [...el.querySelectorAll('input[type=number]')] as HTMLInputElement[]
    expect(nums.length).toBe(2)
    const setVal = (input: HTMLInputElement, v: string) => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
      set.call(input, v)
      input.dispatchEvent(new Event('input', { bubbles: true }))
    }
    await act(async () => { setVal(nums[0], '32768'); setVal(nums[1], '4096') })
    const saveBtn = [...el.querySelectorAll('button')].find((b) => /save|保存|儲存|enregistrer|guardar|salvar/i.test(b.textContent ?? '') && b.classList.contains('primary'))!
    await act(async () => saveBtn.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    const sent = vi.mocked(api.saveProvider).mock.calls.at(-1)![0]
    const m = sent.models?.find((x) => x.id === 'mystery-x')!
    expect(m.context_window).toBe(32768)
    expect(m.max_output).toBe(4096)
    root.unmount()
  })
})

describe('Jev 槽', () => {
  afterEach(() => vi.restoreAllMocks())

  it('第一次保存只绑 jev，不绑项目经理的 decision 槽', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} })
    vi.spyOn(api, 'presetRoles').mockResolvedValue([])
    const bind = vi.spyOn(api, 'setSlotBinding').mockResolvedValue(undefined)
    vi.spyOn(api, 'saveProvider').mockResolvedValue(undefined)
    const { el, root } = await render(<ProviderManager />)
    const row = [...el.querySelectorAll('div')].find((d) => d.textContent === 'TypeSafe')!
    await act(async () => { row.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const save = [...el.querySelectorAll('button')].filter((b) =>
      b.classList.contains('primary') && /save|保存|儲存|enregistrer|guardar|salvar/i.test(b.textContent ?? ''),
    ).at(-1)!
    await act(async () => { save.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(bind).toHaveBeenCalledTimes(1)
    expect(bind).toHaveBeenCalledWith('jev', expect.any(String), 'jev-latest')
    root.unmount()
  })
})
