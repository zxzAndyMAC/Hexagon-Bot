import { describe, expect, it, vi } from 'vitest'
import i18n from './index'
import { setLang } from './index'
import { syncUiLanguageToCore } from './syncCore'

describe('syncUiLanguageToCore', () => {
  it('pushes the current language at startup and every switch', async () => {
    await i18n.changeLanguage('en')
    const push = vi.fn(() => Promise.resolve())
    const stop = syncUiLanguageToCore(i18n, push)
    expect(push).toHaveBeenLastCalledWith('en')
    await setLang('ja')
    expect(push).toHaveBeenLastCalledWith('ja')
    stop()
    await setLang('fr')
    expect(push).not.toHaveBeenCalledWith('fr')
    localStorage.removeItem('hexagon.lang')
    await i18n.changeLanguage('en')
  })

  it('a failed push does not throw into the UI', async () => {
    const push = vi.fn(() => Promise.reject(new Error('no backend')))
    const stop = syncUiLanguageToCore(i18n, push)
    await expect(i18n.changeLanguage('es')).resolves.toBeDefined()
    stop()
    await i18n.changeLanguage('en')
  })
})
