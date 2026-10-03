import { assertModalBlocksIntake } from './modalDecisionTestHelpers'
import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { DesignChoices } from './components/DesignChoices'
import { api, type PendingQuestion } from './api'
import type { DesignDirection } from './gen/DesignDirection'
import { useUiStore } from './store'
import { bindingFor, isMac } from './keymap'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(() => { act(() => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })
const card: PendingQuestion = { id: 'q-design', kind: 'escalation', agent_id: 'a0', state: 'queued', payload: { sub: 'design_direction', revision: 2 } }
const direction: DesignDirection = { project_id: 'p', revision: 2, question_id: 'q-design', state: 'pending', selected_option: null, selected_at: null, existing_guidance: null, options: [
  { id: 'calm', title: 'Calm editorial', description: 'Quiet reading', layout: 'Two columns', typography: 'Serif headings', palette: 'Warm grey', mockups: [{ page: 'Dashboard', mime: 'image/png', digest: 'abc', data_url: 'data:image/png;base64,YQ==' }] },
  { id: 'dense', title: 'Dense dashboard', description: 'Expert controls', layout: 'Grid', typography: 'Sans', palette: 'Ink', mockups: [] },
] }
it('requires an explicit choice and submits the displayed immutable revision', async () => {
  vi.spyOn(api, 'designDirection').mockResolvedValue(direction)
  const choose = vi.spyOn(api, 'chooseDesignDirection').mockResolvedValue({ ...direction, state: 'selected' })
  useUiStore.setState({ refresh: vi.fn().mockResolvedValue(undefined) })
  root = createRoot(el)
  await act(async () => { root.render(<DesignChoices q={card} />) })
  expect(el.querySelector('img')?.getAttribute('src')).toBe(direction.options[0].mockups[0].data_url)
  expect(choose).not.toHaveBeenCalled()
  await act(async () => { el.querySelector<HTMLButtonElement>('article button.primary')!.click() })
  expect(choose).toHaveBeenCalledWith('q-design', 2, 'calm', undefined)
})
it('never offers choices from a superseded card', async () => {
  vi.spyOn(api, 'designDirection').mockResolvedValue({ ...direction, revision: 3, question_id: 'new-q' })
  root = createRoot(el)
  await act(async () => { root.render(<DesignChoices q={card} />) })
  expect(el.querySelector('button')).toBeNull()
  expect(el.querySelector('[role="status"]')).not.toBeNull()
})

it('opens the full mockup without choosing, blocks decisions and restores focus on Escape', async () => {
  vi.spyOn(api, 'designDirection').mockResolvedValue(direction)
  const choose = vi.spyOn(api, 'chooseDesignDirection')
  const pushToast = vi.fn()
  useUiStore.setState({ pushToast })
  document.body.append(el)
  root = createRoot(el)
  await act(async () => { root.render(<DesignChoices q={card} />) })
  const trigger = el.querySelector<HTMLButtonElement>('.design-mockup-trigger')!
  // WebKit does not always focus pointer-clicked buttons; the opener must do so.
  await act(async () => trigger.click())
  const dialog = el.querySelector('dialog')!
  expect(dialog.hasAttribute('open')).toBe(true)
  expect(dialog.querySelector('img')?.getAttribute('src')).toBe(direction.options[0].mockups[0].data_url)
  expect(document.activeElement).toBe(dialog.querySelector('button'))
  const background = vi.fn()
  window.addEventListener('keydown', background)
  try {
    for (const key of ['Enter', 'Backspace']) {
      await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key, ctrlKey: !isMac, metaKey: isMac, bubbles: true, cancelable: true })))
    }
    expect(background).not.toHaveBeenCalled()
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: !isMac, metaKey: isMac, altKey: true, shiftKey: true, bubbles: true, cancelable: true })))
    expect(background).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledTimes(3)
    expect(choose).not.toHaveBeenCalled()
    await assertModalBlocksIntake()
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })))
    expect(el.querySelector('dialog')).toBeNull()
    expect(document.activeElement).toBe(trigger)
    expect(background).not.toHaveBeenCalled()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: !isMac, metaKey: isMac }))
    expect(background).toHaveBeenCalledTimes(1)
  } finally { window.removeEventListener('keydown', background) }
  expect(bindingFor('closeDesignPreview')).toBe('Escape')
})

it('closes through its button or native cancel without choosing a direction', async () => {
  vi.spyOn(api, 'designDirection').mockResolvedValue(direction)
  const choose = vi.spyOn(api, 'chooseDesignDirection')
  document.body.append(el)
  root = createRoot(el)
  await act(async () => { root.render(<DesignChoices q={card} />) })
  const trigger = el.querySelector<HTMLButtonElement>('.design-mockup-trigger')!
  for (const cancel of [false, true]) {
    trigger.focus()
    await act(async () => trigger.click())
    await act(async () => {
      if (cancel) el.querySelector('dialog')!.dispatchEvent(new Event('cancel', { cancelable: true }))
      else el.querySelector<HTMLButtonElement>('dialog button')!.click()
    })
    expect(el.querySelector('dialog')).toBeNull()
    expect(document.activeElement).toBe(trigger)
  }
  expect(choose).not.toHaveBeenCalled()
})

it('removes the keyboard guard on unmount without replacing another modal scope', async () => {
  vi.spyOn(api, 'designDirection').mockResolvedValue(direction)
  useUiStore.setState({ modalScope: 'settings' })
  root = createRoot(el)
  await act(async () => { root.render(<DesignChoices q={card} />) })
  await act(async () => el.querySelector<HTMLButtonElement>('.design-mockup-trigger')!.click())
  await act(async () => root.render(null))
  expect(useUiStore.getState().modalScope).toBe('settings')
  const key = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: !isMac, metaKey: isMac, cancelable: true })
  window.dispatchEvent(key)
  expect(key.defaultPrevented).toBe(false)
  useUiStore.setState({ modalScope: 'workbench' })
})
