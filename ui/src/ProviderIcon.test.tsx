// owner-followups-1015 #23（followups 第 12 项）：服务商品牌图标的查找、深浅切换与中性兜底（components A13b）。
// 资产本体的 sha256 / 完整性在 providerIcons.assets.test.ts；这里用注入的假资产表，
// 断言不依赖随包 SVG 是否已落盘。
import { describe, expect, it, vi, afterEach, beforeEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { api } from './api'
import { ProviderManager } from './components/ProviderManager'
import { ProviderIcon } from './components/ProviderIcon'
import { iconKeyOf, providerIconAsset } from './providerIcons'
import type { ProviderPreset } from './gen/ProviderPreset'
import type { ProvidersView } from './gen/ProvidersView'
import { prov, renderWizard, unmount } from './wizardTestUtils'

const FAKE = vi.hoisted(() => ({
  './assets/providers/deepseek-light.svg': '/deepseek-light.svg',
  './assets/providers/openai-light.svg': '/openai-light.svg',
  './assets/providers/openai-dark.svg': '/openai-dark.svg',
  './assets/providers/anthropic.svg': '/anthropic.svg',
  './assets/providers/anthropic-dark.svg': '/anthropic-dark.svg',
  './assets/providers/lmstudio.svg': '/lmstudio.svg',
}) as Record<string, string>)

// 组件走默认资产表（import.meta.glob）；测试把默认表换成假表，其余实现原样。
vi.mock('./providerIcons', async (importOriginal) => {
  const real = await importOriginal<typeof import('./providerIcons')>()
  return { ...real, providerIconAsset: (key: string, urls?: Record<string, string>) => real.providerIconAsset(key, urls ?? FAKE) }
})

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const preset = (p: Partial<ProviderPreset> & Pick<ProviderPreset, 'id' | 'name'>): ProviderPreset => ({
  kind: 'openai', base_url: `https://${p.id}.example/v1`, default_model: '', icon_key: '', website: '', docs: '',
  sample_models: [], origin: 'catalog', ...p,
})

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

const imgs = (n: Element) => [...n.querySelectorAll('img')].map((i) => [i.getAttribute('src'), i.dataset.themeVariant, i.className])

describe('providerIconAsset：icon_key → 深浅资产（A13b）', () => {
  it('浅深两版各取各的', () => {
    expect(providerIconAsset('openai', FAKE)).toEqual({ light: '/openai-light.svg', dark: '/openai-dark.svg' })
  })
  it('缺深色用浅色', () => {
    expect(providerIconAsset('deepseek', FAKE)).toEqual({ light: '/deepseek-light.svg', dark: '/deepseek-light.svg' })
  })
  it('单色图（无 -light 后缀）当浅色，配有 -dark 时深色取 -dark', () => {
    expect(providerIconAsset('lmstudio', FAKE)).toEqual({ light: '/lmstudio.svg', dark: '/lmstudio.svg' })
    expect(providerIconAsset('anthropic', FAKE)).toEqual({ light: '/anthropic.svg', dark: '/anthropic-dark.svg' })
  })
  it('空键或资产缺失 = null（中性图标）', () => {
    expect(providerIconAsset('', FAKE)).toBeNull()
    expect(providerIconAsset('nope', FAKE)).toBeNull()
  })
  it('iconKeyOf 只按 id 对预设，不按名称', () => {
    const presets = [preset({ id: 'openai', name: 'OpenAI', icon_key: 'openai' })]
    expect(iconKeyOf(presets, 'openai')).toBe('openai')
    expect(iconKeyOf(presets, 'my-gateway')).toBe('')
    expect(iconKeyOf(presets, undefined)).toBe('')
  })
})

describe('ProviderIcon 渲染', () => {
  it('有深色版：两张图同挂，按 data-theme 的 dark: 变体切显隐', async () => {
    const { el, root } = await render(<ProviderIcon iconKey="openai" size={28} />)
    const box = el.querySelector('[data-provider-icon="openai"]')!
    expect(box.getAttribute('aria-hidden')).toBe('true')
    expect(box.className).toContain('size-7')
    expect(imgs(box)).toEqual([
      ['/openai-light.svg', 'light', expect.stringContaining('dark:hidden')],
      ['/openai-dark.svg', 'dark', expect.stringContaining('hidden dark:block')],
    ])
    root.unmount()
  })
  it('无深色版：只一张图，深浅都显示', async () => {
    const { el, root } = await render(<ProviderIcon iconKey="deepseek" size={16} />)
    const box = el.querySelector('[data-provider-icon="deepseek"]')!
    expect(box.className).toContain('size-4')
    expect(imgs(box)).toEqual([['/deepseek-light.svg', 'light', expect.not.stringContaining('hidden')]])
    root.unmount()
  })
  it('无图标：中性图标块，不画任何图片', async () => {
    const { el, root } = await render(<ProviderIcon iconKey="" />)
    const box = el.querySelector('[data-provider-icon="neutral"]')!
    expect(box.className).toContain('size-5')
    expect(box.querySelector('img')).toBeNull()
    expect(box.querySelector('svg')).not.toBeNull()
    root.unmount()
  })
})

describe('设置「模型服务」：列表 / 详情 / 槽位表的图标', () => {
  const pv: ProvidersView = {
    providers: [
      { id: 'deepseek', name: 'DeepSeek', kind: 'openai', base_url: 'https://api.deepseek.com', models: [], enabled: true, key_set: true },
      // 自建网关起名 OpenAI、走 openai 协议——仍是中性图标（不按名称或协议套品牌）
      { id: 'my-gateway', name: 'OpenAI', kind: 'openai', base_url: 'https://gw.example/v1', models: [], enabled: true, key_set: true },
    ],
    slots: { default: { provider_id: 'deepseek', model: 'deepseek-chat' }, chat: { provider_id: 'my-gateway', model: 'x' } },
  }
  const presets = [
    preset({ id: 'deepseek', name: 'DeepSeek', icon_key: 'deepseek' }),
    preset({ id: 'openai', name: 'OpenAI Official', icon_key: 'openai' }),
    preset({ id: 'jev', name: 'Jev', kind: 'jev', icon_key: '' }),
  ]
  beforeEach(() => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(pv)
    vi.spyOn(api, 'presetRoles').mockResolvedValue([])
    vi.spyOn(api, 'providerPresets').mockResolvedValue(presets)
  })
  afterEach(() => vi.restoreAllMocks())

  // 行外层与名字 div 的 textContent 相同；取最内层（名字本身）再看同一行里的图标
  const nameDiv = (el: HTMLElement, name: string) =>
    [...el.querySelectorAll('div')].find((d) => d.textContent === name && !d.querySelector('div'))!
  const iconBeside = (el: HTMLElement, name: string) => nameDiv(el, name).parentElement!.querySelector('[data-provider-icon]')!

  it('左列：已配置按 id 对上预设出品牌图，自定义与无图标预设出中性图标；尺寸 20', async () => {
    const { el, root } = await render(<ProviderManager />)
    expect(iconBeside(el, 'DeepSeek').getAttribute('data-provider-icon')).toBe('deepseek')
    expect(iconBeside(el, 'OpenAI').getAttribute('data-provider-icon')).toBe('neutral')
    expect(iconBeside(el, 'OpenAI Official').getAttribute('data-provider-icon')).toBe('openai')
    expect(iconBeside(el, 'Jev').getAttribute('data-provider-icon')).toBe('neutral')
    expect(iconBeside(el, 'DeepSeek').className).toContain('size-5')
    root.unmount()
  })

  it('详情头 28 图标随选中项；槽位表在下拉前挂所选服务商的 16 图标', async () => {
    const { el, root } = await render(<ProviderManager />)
    const row = nameDiv(el, 'DeepSeek')
    await act(async () => { row.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    const head = el.querySelector('[data-provider-detail] [data-provider-icon]')!
    expect(head.getAttribute('data-provider-icon')).toBe('deepseek')
    expect(head.className).toContain('size-7')
    const slotIcon = (slot: string) => el.querySelector(`[data-slot-row="${slot}"] [data-provider-icon]`)
    expect(slotIcon('default')!.getAttribute('data-provider-icon')).toBe('deepseek')
    expect(slotIcon('default')!.className).toContain('size-4')
    expect(slotIcon('chat')!.getAttribute('data-provider-icon')).toBe('neutral')
    root.unmount()
  })
})

describe('向导服务商下拉的图标', () => {
  const prevLang = i18n.language
  beforeEach(async () => {
    localStorage.clear()
    await i18n.changeLanguage('en')
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([])
    vi.spyOn(api, 'presetPacks').mockResolvedValue([])
  })
  afterEach(async () => {
    await unmount()
    vi.restoreAllMocks()
    document.body.replaceChildren()
    await i18n.changeLanguage(prevLang)
  })

  it('已配置项按 id 对完整预设表取图标（即便该预设因同 id 不再列为内置项）；预设项用自己的 icon_key', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue({
      providers: [prov({ id: 'deepseek', name: 'DeepSeek' }), prov({ id: 'gw', name: 'My gateway' })],
      slots: {},
    })
    vi.spyOn(api, 'providerPresets').mockResolvedValue([
      preset({ id: 'anthropic', name: 'Anthropic', kind: 'anthropic', icon_key: 'anthropic', origin: 'manual' }),
      preset({ id: 'deepseek', name: 'DeepSeek', icon_key: 'deepseek' }),
    ])
    const el = await renderWizard()
    await act(async () => {
      el.querySelector('[data-provider-select]')!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })
    const icon = (label: string) => [...document.querySelectorAll('[role="option"]')]
      .find((o) => o.querySelector('[data-select-text]')?.textContent === label)!
      .querySelector('[data-provider-icon]')?.getAttribute('data-provider-icon')
    expect(icon('DeepSeek')).toBe('deepseek')
    expect(icon('My gateway')).toBe('neutral')
    expect(icon('Anthropic')).toBe('anthropic')
  })
})
