import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import './i18n'
import { api } from './api'
import { PackEditor } from './components/PackEditor'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

it('preserves explicit quality contracts and quoted commands in browser YAML export', async () => {
  const command = 'node checks/a11y.mjs --label "contrast: AA"'
  vi.spyOn(api, 'packDraft').mockResolvedValue({ name: 'Quality', version: 1,
    knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
    stages: [{ name: 'Build', roles: [], due: [], checks: [], reviews: [], stamp_point: true,
      backfill_edges: [], consult_wake: [], quality_checks: { accessibility: command } }],
  })
  vi.spyOn(api, 'packTemplates').mockResolvedValue([])
  let download: Blob | undefined
  vi.spyOn(URL, 'createObjectURL').mockImplementation(blob => { download = blob as Blob; return 'blob:quality' })
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
  const el = document.createElement('div')
  const root = createRoot(el)
  try {
    await act(async () => { root.render(<PackEditor onClose={() => {}} />) })
    expect(el.textContent).toContain('quality_checks')
    const button = [...el.querySelectorAll('button')].find(b => b.textContent === 'Export YAML')!
    await act(async () => { button.click() })
    const yaml = await download!.text()
    expect(yaml).toContain('    quality_checks:\n')
    expect(yaml).toContain(`      "accessibility": ${JSON.stringify(command)}\n`)
  } finally {
    await act(async () => root.unmount())
    vi.restoreAllMocks()
  }
})
