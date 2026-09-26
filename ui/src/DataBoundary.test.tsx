import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api } from './api'
import { SettingsPage } from './components/SettingsPage'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
it.each([true, false])('shows actual plaintext storage and distinct data paths (projectless=%s)', async (projectless) => {
  await i18n.changeLanguage('en')
  const query = vi.spyOn(api, 'dataBoundary').mockResolvedValue({ credential_backend: 'dev_file', project_open: !projectless,
    recipients: [{ kind: 'model', endpoint: 'https://model.example', transport: 'http', enabled: true },
      { kind: 'mcp', endpoint: null, transport: 'stdio', enabled: true }] })
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<SettingsPage projectless={projectless} onBack={() => {}} />))
    expect(query).toHaveBeenCalledOnce()
    expect(el.textContent).toContain('Local plaintext file')
    expect(el.textContent).not.toContain('System credential store')
    expect(el.textContent).toContain('https://model.example')
    expect(el.textContent).toContain('does not block model or MCP requests')
    expect(el.textContent).toContain('does not automatically erase history')
    expect(el.textContent).toContain('external recipients depend on the service')
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})

it('shows unknown recipients when configuration cannot be read', async () => {
  vi.spyOn(api, 'dataBoundary').mockRejectedValue({ code: 'data_boundary_unavailable', message: 'SECRET_CONFIG' })
  const el = document.createElement('div'); const root = createRoot(el)
  try {
    await act(async () => root.render(<SettingsPage projectless onBack={() => {}} />))
    expect(el.textContent).toContain('recipients are unknown')
    expect(el.textContent).not.toContain('SECRET_CONFIG')
    expect(el.textContent).not.toContain('No configured recipients')
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks() }
})
