import { describe, expect, it, vi, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { PromptsSection } from './components/PromptsSection'
import { api } from './api'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const CATALOG = [
  { id: 'workbench.base', group: 'workbench', text: '# Hexagon workbench', hash: 'a' },
  { id: 'tool.fs_patch', group: 'tools', text: 'Replace an exact string', hash: 'b' },
]

async function render(onGoModels = () => {}) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(<PromptsSection onGoModels={onGoModels} />) })
  await act(async () => { await Promise.resolve() })
  return { el, root }
}

describe('PromptsSection（prompt-engineering 票 11）', () => {
  afterEach(async () => {
    vi.restoreAllMocks()
    await i18n.changeLanguage('en')
  })

  it('英文界面只显示原文，不发翻译请求', async () => {
    await i18n.changeLanguage('en')
    vi.spyOn(api, 'promptCatalog').mockResolvedValue(CATALOG)
    const tr = vi.spyOn(api, 'translatePrompts')
    const { el, root } = await render()
    expect(el.textContent).toContain('# Hexagon workbench')
    expect(el.querySelector('[data-translation]')).toBeNull()
    expect(tr).not.toHaveBeenCalled()
    act(() => root.unmount())
  })

  it('非英文界面：原文与参考译文并排，重新翻译带 force', async () => {
    await i18n.changeLanguage('zh-CN')
    vi.spyOn(api, 'promptCatalog').mockResolvedValue(CATALOG)
    const tr = vi.spyOn(api, 'translatePrompts').mockResolvedValue({
      no_model: false,
      entries: [
        { id: 'workbench.base', text: '# Hexagon 工作台', error: null },
        { id: 'tool.fs_patch', text: '替换一段精确字符串', error: null },
      ],
    })
    const { el, root } = await render()
    expect(tr).toHaveBeenCalledWith('zh-CN', false)
    const translated = [...el.querySelectorAll('[data-translation]')].map((n) => n.textContent)
    expect(translated).toEqual(['# Hexagon 工作台', '替换一段精确字符串'])
    expect(el.textContent).toContain('参考译文')
    const again = [...el.querySelectorAll('button')].find((b) => b.textContent === '重新翻译')!
    await act(async () => { again.click() })
    expect(tr).toHaveBeenLastCalledWith('zh-CN', true)
    act(() => root.unmount())
  })

  it('切语言时丢弃上一种语言迟到的译文', async () => {
    await i18n.changeLanguage('ja')
    vi.spyOn(api, 'promptCatalog').mockResolvedValue(CATALOG)
    let releaseJa: (v: { no_model: boolean; entries: { id: string; text: string | null; error: string | null }[] }) => void = () => {}
    vi.spyOn(api, 'translatePrompts').mockImplementation((lang: string) =>
      lang === 'ja'
        ? new Promise((r) => { releaseJa = r })
        : Promise.resolve({ no_model: false, entries: [{ id: 'workbench.base', text: 'FR', error: null }, { id: 'tool.fs_patch', text: 'FR2', error: null }] }),
    )
    const { el, root } = await render()
    await act(async () => { await i18n.changeLanguage('fr') })
    await act(async () => { releaseJa({ no_model: false, entries: [{ id: 'workbench.base', text: 'JA', error: null }, { id: 'tool.fs_patch', text: 'JA2', error: null }] }) })
    const translated = [...el.querySelectorAll('[data-translation]')].map((n) => n.textContent)
    expect(translated).toEqual(['FR', 'FR2'])
    act(() => root.unmount())
  })

  it('没有可用模型：整页英文 + 去模型设置；某组失败只在该组提示', async () => {
    await i18n.changeLanguage('ja')
    vi.spyOn(api, 'promptCatalog').mockResolvedValue(CATALOG)
    vi.spyOn(api, 'translatePrompts').mockResolvedValue({ no_model: true, entries: [] })
    const go = vi.fn()
    const first = await render(go)
    expect(first.el.textContent).toContain('# Hexagon workbench')
    expect(first.el.querySelector('[data-translation]')).toBeNull()
    const btn = [...first.el.querySelectorAll('button')].find((b) => b.textContent === 'モデルサービスを開く')!
    await act(async () => { btn.click() })
    expect(go).toHaveBeenCalled()
    act(() => first.root.unmount())

    vi.spyOn(api, 'translatePrompts').mockResolvedValue({
      no_model: false,
      entries: [
        { id: 'workbench.base', text: null, error: 'not_json' },
        { id: 'tool.fs_patch', text: '正確な文字列を置換', error: null },
      ],
    })
    const second = await render()
    const alerts = [...second.el.querySelectorAll('[role="alert"]')].map((n) => n.textContent)
    // 后端只给原因码，文案由界面按语言给出（code-review：别把英文原句漏给负责人）。
    expect(alerts).toEqual(['翻訳に失敗しました：翻訳結果の形式が正しくありません'])
    expect(second.el.textContent).toContain('正確な文字列を置換')
    act(() => second.root.unmount())
  })
})
