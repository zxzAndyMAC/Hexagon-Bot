// 主题切片（ui-redesign review S4，2026-10-17 从 store.ts 拆出，行为不变——
// store.ts 超 800 行上限）。偏好存 localStorage，`system` 档跟随 OS 切换。
export type ThemePref = 'light' | 'dark' | 'night' | 'system'

const THEME_KEY = 'hexagon.theme'
const DARK_QUERY = '(prefers-color-scheme: dark)'

function resolveTheme(pref: ThemePref): 'light' | 'dark' | 'night' {
  if (pref !== 'system') return pref
  return matchMedia(DARK_QUERY).matches ? 'dark' : 'light'
}

function applyTheme(pref: ThemePref) {
  document.documentElement.dataset.theme = resolveTheme(pref)
}

export interface ThemeSlice {
  themePref: ThemePref
  setThemePref: (p: ThemePref) => void
}

/// 模块加载即生效（首帧不闪主题），并挂上 OS 主题监听；`current` 读切片当前偏好。
export function themeSlice(set: (partial: Pick<ThemeSlice, 'themePref'>) => void, current: () => ThemePref): ThemeSlice {
  const saved = (localStorage.getItem(THEME_KEY) as ThemePref) || 'dark'
  applyTheme(saved)
  matchMedia(DARK_QUERY).addEventListener('change', () => {
    if (current() === 'system') applyTheme('system')
  })
  return {
    themePref: saved,
    setThemePref: (p) => {
      localStorage.setItem(THEME_KEY, p)
      applyTheme(p)
      set({ themePref: p })
    },
  }
}
