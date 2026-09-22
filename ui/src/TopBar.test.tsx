import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { TopBar } from './components/TopBar'
import { api } from './api'
import { useUiStore } from './store'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

describe('TopBar 自治下拉（票 01）', () => {
  beforeEach(() => {
    useUiStore.setState({ autonomy: 'L2', projectName: 'p', pending: [], team: [] })
    vi.spyOn(api, 'sandboxStatus').mockResolvedValue({ available: false, mode: 'none', note: '' })
  })
  afterEach(() => vi.restoreAllMocks())

  it('列出 L0–L4，改档后读回同一档；拒绝时下拉回到原档', async () => {
    const setAutonomy = vi.spyOn(api, 'setAutonomy').mockResolvedValue(undefined)
    vi.spyOn(api, 'autonomy').mockResolvedValue('L4')
    vi.spyOn(api, 'projectInfo').mockResolvedValue({
      name: 'p', mode: 'pack', pack_name: null, fastpath_role: null,
    } as never)
    const { el, root } = await render(<TopBar onSettings={() => {}} onProjectClosed={() => {}} />)
    const sel = el.querySelector('select[aria-label="Change autonomy level"]') as HTMLSelectElement
    expect(sel).toBeTruthy()
    const values = [...sel.options].map((o) => o.value)
    expect(values).toEqual(['L0', 'L1', 'L2', 'L3', 'L4'])
    expect(sel.selectedOptions[0].textContent).toContain('L2')
    expect([...sel.options].some((o) => o.textContent?.startsWith('Autonomy L3'))).toBe(true)
    expect([...sel.options].some((o) => o.textContent?.startsWith('Autonomy L4'))).toBe(true)

    await act(async () => {
      sel.value = 'L4'
      sel.dispatchEvent(new Event('change', { bubbles: true }))
    })
    expect(setAutonomy).toHaveBeenCalledWith('L4')
    expect(useUiStore.getState().autonomy).toBe('L4')

    setAutonomy.mockRejectedValueOnce(new Error('nope'))
    await act(async () => {
      sel.value = 'L0'
      sel.dispatchEvent(new Event('change', { bubbles: true }))
    })
    expect(sel.value).toBe('L4')
    root.unmount()
  })
})
