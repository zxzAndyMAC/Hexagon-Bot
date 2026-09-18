import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from './locales/en'
import zhCN from './locales/zh-CN'
import zhTW from './locales/zh-TW'
import ja from './locales/ja'
import es from './locales/es'
import pt from './locales/pt'
import fr from './locales/fr'

export const SUPPORTED = ['zh-CN', 'zh-TW', 'en', 'ja', 'es', 'pt', 'fr'] as const
export type Locale = (typeof SUPPORTED)[number]

const STORAGE_KEY = 'hexagon.lang'

// 手动选过的语言持久化；否则跟随系统语言，不支持 → en。
// zh 按书写系统归位：Hant/TW/HK/MO → 繁中，其余 → 简中。
export function detectLang(sysOverride?: string): Locale {
  const stored = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null
  const sys =
    sysOverride ??
    (typeof navigator !== 'undefined' ? navigator.languages?.[0] ?? navigator.language : null)
  const raw = stored ?? sys ?? 'en'
  const c = raw.toLowerCase()
  if (c.startsWith('zh')) return /hant|tw|hk|mo/.test(c) ? 'zh-TW' : 'zh-CN'
  for (const l of ['en', 'ja', 'es', 'pt', 'fr'] as const) {
    if (c === l || c.startsWith(`${l}-`)) return l
  }
  return 'en'
}

export function setLang(l: Locale) {
  localStorage.setItem(STORAGE_KEY, l)
  return i18n.changeLanguage(l)
}

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    'zh-CN': { translation: zhCN },
    'zh-TW': { translation: zhTW },
    ja: { translation: ja },
    es: { translation: es },
    pt: { translation: pt },
    fr: { translation: fr },
  },
  lng: detectLang(),
  supportedLngs: [...SUPPORTED],
  fallbackLng: 'en', // 缺译 key → 英文
  interpolation: { escapeValue: false },
})

export default i18n
