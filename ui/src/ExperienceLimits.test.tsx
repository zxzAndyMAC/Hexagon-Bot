import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import './i18n'
import { api } from './api'
import { ExperienceLimits } from './components/ExperienceLimits'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
afterEach(() => vi.restoreAllMocks())
it('loads persisted limits, submits typed values, and retains rejected input with an error', async () => {
  const limits = { entry_chars: 2000, load_count: 10, load_chars: 8000 }
  vi.spyOn(api, 'experienceLimits').mockResolvedValue(limits)
  const save = vi.spyOn(api, 'setExperienceLimits').mockRejectedValue(new Error('Rejected budget'))
  const onSaved = vi.fn()
  const host = document.createElement('div')
  const root = createRoot(host)
  await act(async () => { root.render(<ExperienceLimits onSaved={onSaved} />) })
  expect(Array.from(host.querySelectorAll('input')).map((i) => i.value)).toEqual(['2000', '10', '8000'])
  await act(async () => { host.querySelector('button')?.click() })
  expect(save).toHaveBeenCalledWith(limits)
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('Rejected budget')
  expect(onSaved).not.toHaveBeenCalled()
  save.mockResolvedValue(limits)
  await act(async () => { host.querySelector('button')?.click() })
  expect(onSaved).toHaveBeenCalledOnce()
  expect(host.querySelector('[role="alert"]')).toBeNull()
  await act(async () => root.unmount())
})
