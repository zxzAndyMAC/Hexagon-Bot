import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api } from './api'
import { useUiStore } from './store'
import { CommandPalette } from './components/CommandPalette'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it.each(['检查', '测试', '检验'])('lets a Chinese owner find and execute quality checks by %s', async (query) => {
  const language = i18n.language
  const saved = useUiStore.getState()
  const recents = localStorage.getItem('hexagon.palette.recents')
  await i18n.changeLanguage('zh-CN')
  useUiStore.setState({ team: [], artifacts: [], pending: [], invalidate: async () => {} })
  localStorage.removeItem('hexagon.palette.recents')
  const run = vi.spyOn(api, 'runChecks').mockResolvedValue({ results: [] })
  const el = document.createElement('div')
  document.body.append(el)
  const root = createRoot(el)
  try {
    await act(async () => root.render(<CommandPalette onClose={() => {}} />))
    const input = el.querySelector('input')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, query)
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    const option = el.querySelector('[role="option"]')
    expect(option?.textContent).toBe('跑检验')
    await act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
    expect(run).toHaveBeenCalledExactlyOnceWith()
  } finally {
    await act(async () => root.unmount())
    el.remove()
    vi.restoreAllMocks()
    useUiStore.setState(saved, true)
    await i18n.changeLanguage(language)
    if (recents === null) localStorage.removeItem('hexagon.palette.recents')
    else localStorage.setItem('hexagon.palette.recents', recents)
  }
})
