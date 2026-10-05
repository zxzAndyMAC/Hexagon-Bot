import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { api } from './api'
import i18n from './i18n'
import { QualityCommands } from './components/QualityCommands'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it('reviews affected current-flow commands before confirming the displayed version', async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'qualityConfiguration').mockResolvedValue({ version: 'config:1', project_root: '/project-one', stages: [{ seq: 0, name: 'Delivery', commands: { tests: 'sh old.sh' } }] })
  const update = vi.spyOn(api, 'updateQualityCommands').mockResolvedValue({ version: 'config:2', project_root: '/project-one', stages: [{ seq: 0, name: 'Delivery', commands: { tests: 'sh new.sh' } }] })
  const run = vi.spyOn(api, 'runChecks').mockResolvedValue({ results: [{ cmd: 'quality:tests', exit_code: 7 }] })
  const el = document.createElement('div')
  const root = createRoot(el)
  try {
    await act(async () => root.render(<QualityCommands />))
    const input = el.querySelector<HTMLInputElement>('input[data-quality-category="tests"]')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'sh new.sh')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(update).not.toHaveBeenCalled()
    await act(async () => el.querySelector<HTMLButtonElement>('[data-review-quality]')!.click())
    expect(el.textContent).toContain('sh old.sh')
    expect(el.textContent).toContain('sh new.sh')
    expect(el.textContent).toContain(i18n.t('quality.configImpact'))
    expect(update).not.toHaveBeenCalled()
    await act(async () => el.querySelector<HTMLButtonElement>('[data-confirm-quality]')!.click())
    expect(update).toHaveBeenCalledWith(0, 'config:1', { tests: 'sh new.sh' }, '/project-one')
    expect(el.textContent).toContain(i18n.t('quality.configSaved'))
    await act(async () => [...el.querySelectorAll('button')].find(button => button.textContent === i18n.t('quality.configRun'))!.click())
    expect(run).toHaveBeenCalledWith('/project-one')
    expect(el.textContent).toContain(i18n.t('quality.configChecked'))
    expect(el.textContent).not.toContain('All passed')
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})
