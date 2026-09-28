import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import './i18n'
import { api } from './api'
import { ExperienceCuration } from './components/ExperienceCuration'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
it('requires a selected original range and verified review reference before submitting a draft', async () => {
  vi.stubGlobal('crypto', { subtle: { digest: vi.fn(async () => new Uint8Array(32).buffer) } })
  const submit = vi.spyOn(api, 'curateLegacyExperience').mockResolvedValue({ proposal_id: 'curated1' })
  const content = '# Instructions\n## 经验\nLegacy line\n'
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  await act(async () => root.render(<ExperienceCuration document={{ project_root: '/p', skill: 'alpha', digest: 'file-version', content }} />))
  expect(host.querySelector('button')?.disabled).toBe(true)
  const original = host.querySelector('textarea[readonly]')!
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!
  await act(async () => {
    const inputs = host.querySelectorAll('input')
    setter.call(inputs[0], '12'); inputs[0].dispatchEvent(new Event('input', { bubbles: true }))
    setter.call(inputs[1], 'This skill owns the lesson'); inputs[1].dispatchEvent(new Event('input', { bubbles: true }))
    ;(original as HTMLTextAreaElement).focus()
    ;(original as HTMLTextAreaElement).setSelectionRange(content.indexOf('Legacy'), content.length)
    original.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
  })
  expect(host.querySelector('button')?.disabled).toBe(false)
  await act(async () => host.querySelector('button')?.click())
  expect(submit).toHaveBeenCalledWith(expect.objectContaining({ project_root: '/p', review_event: 12, legacy: expect.objectContaining({ skill: 'alpha' }), request: expect.objectContaining({ body: 'Legacy line' }) }))
  await act(async () => root.unmount())
  host.remove()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})
