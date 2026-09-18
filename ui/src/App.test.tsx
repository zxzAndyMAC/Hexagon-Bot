import { describe, expect, it } from 'vitest'
import { useUiStore } from './store'
import i18n, { SUPPORTED, detectLang, setLang } from './i18n'
import en from './i18n/locales/en'
import zhCN from './i18n/locales/zh-CN'
import zhTW from './i18n/locales/zh-TW'
import ja from './i18n/locales/ja'
import es from './i18n/locales/es'
import pt from './i18n/locales/pt'
import fr from './i18n/locales/fr'

const flatten = (o: Record<string, unknown>, prefix = ''): string[] =>
  Object.entries(o).flatMap(([k, v]) =>
    v && typeof v === 'object' ? flatten(v as Record<string, unknown>, `${prefix}${k}.`) : [`${prefix}${k}`],
  )

const LOCALES = { en, 'zh-CN': zhCN, 'zh-TW': zhTW, ja, es, pt, fr }

describe('i18n dictionaries', () => {
  it('has exactly the seven supported locales', () => {
    expect([...SUPPORTED].sort()).toEqual(['en', 'es', 'fr', 'ja', 'pt', 'zh-CN', 'zh-TW'])
  })

  it('every locale covers every key en defines — no exposed keys', () => {
    const enKeys = flatten(en as unknown as Record<string, unknown>).sort()
    for (const [name, dict] of Object.entries(LOCALES)) {
      const keys = flatten(dict as unknown as Record<string, unknown>).sort()
      expect(keys, `locale ${name}`).toEqual(enKeys)
    }
  })

  it('detects system language with en fallback and zh script variants', () => {
    localStorage.removeItem('hexagon.lang')
    const cases: [string, string][] = [
      ['zh-CN', 'zh-CN'], ['zh-Hans-SG', 'zh-CN'], ['zh-SG', 'zh-CN'],
      ['zh-TW', 'zh-TW'], ['zh-Hant-HK', 'zh-TW'], ['zh-MO', 'zh-TW'],
      ['ja-JP', 'ja'], ['es-ES', 'es'], ['pt-BR', 'pt'], ['fr-FR', 'fr'],
      ['de-DE', 'en'], ['ko-KR', 'en'], // 不支持 → 英文
    ]
    for (const [sys, want] of cases) {
      expect(detectLang(sys), sys).toBe(want)
    }
    // 手动选择过的语言优先于系统
    localStorage.setItem('hexagon.lang', 'fr')
    expect(detectLang('ja-JP')).toBe('fr')
    localStorage.removeItem('hexagon.lang')
  })

  it('missing keys fall back to en', () => {
    expect(i18n.options.fallbackLng).toContain('en')
    // 字典奇偶测试已保证七语全覆盖；此处验证解析链末端一定是 en
    const codes = i18n.services.languageUtils.toResolveHierarchy('fr')
    expect(codes[codes.length - 1]).toBe('en')
  })

  it('manual language choice persists', async () => {
    await setLang('fr')
    expect(localStorage.getItem('hexagon.lang')).toBe('fr')
    localStorage.removeItem('hexagon.lang')
    await i18n.changeLanguage('en')
  })

  it('resolves a key after switching language', async () => {
    await i18n.changeLanguage('zh-CN')
    expect(i18n.t('topbar.sleepAll')).toBe('全员休眠')
    await i18n.changeLanguage('fr')
    expect(i18n.t('topbar.sleepAll')).toBe('Tout suspendre')
    await i18n.changeLanguage('en')
    expect(i18n.t('topbar.sleepAll')).toBe('Sleep all')
  })
})

describe('ui store', () => {
  it('theme pref persists and resolves to <html>', () => {
    const store = useUiStore.getState()
    store.setThemePref('light')
    expect(useUiStore.getState().themePref).toBe('light')
    expect(document.documentElement.dataset.theme).toBe('light')
    expect(localStorage.getItem('hexagon.theme')).toBe('light')
    // system 档按 prefers-color-scheme 解析
    store.setThemePref('system')
    expect(['light', 'dark']).toContain(document.documentElement.dataset.theme)
    store.setThemePref('dark')
  })
})
