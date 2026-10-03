import { assertModalBlocksIntake } from './modalDecisionTestHelpers'
import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { ApprovalModeSelector } from './components/ApprovalModeSelector'
import './i18n'
import { setBinding, resetBinding } from './keymap'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(() => { root?.unmount(); el.replaceChildren(); el.remove() })
it('uses persisted value, offers three modes and supports keyboard dismissal', async () => {
  root = createRoot(el)
  document.body.appendChild(el)
  const change = vi.fn().mockResolvedValue(undefined)
  await act(async () => { root.render(<ApprovalModeSelector value="restricted" onChange={change} />) })
  const trigger = el.querySelector('button')!
  await act(async () => { trigger.click() })
  const options = el.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')
  expect(options).toHaveLength(3)
  expect(options[0].getAttribute('aria-checked')).toBe('true')
  expect(document.activeElement).toBe(options[0])
  await act(async () => { options[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })) })
  expect(document.activeElement).toBe(options[1])
  await act(async () => { options[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) })
  expect(el.querySelector('[role="menu"]')).toBeNull()
  expect(document.activeElement).toBe(trigger)
  expect(change).not.toHaveBeenCalled()
})
it('waits for persistence, blocks duplicates and keeps current value when saving fails', async () => {
  root = createRoot(el)
  let reject!: (reason: Error) => void
  const change = vi.fn(() => new Promise<void>((_, no) => { reject = no }))
  await act(async () => { root.render(<ApprovalModeSelector value="restricted" onChange={change} />) })
  await act(async () => { el.querySelector('button')!.click() })
  const broad = el.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')[2]
  // Issue 10: selecting Broad now opens consent; only its explicit action saves.
  await act(async () => { broad.click() })
  const confirm = el.querySelectorAll<HTMLButtonElement>('[role=alertdialog] button')[1]
  await act(async () => { confirm.click(); confirm.click() })
  expect(change).toHaveBeenCalledTimes(1)
  expect(change).toHaveBeenCalledWith('broad')
  expect(el.querySelector('.approval-mode-trigger')?.textContent).not.toContain('Broad access')
  expect(el.querySelector('[role="alertdialog"]')?.getAttribute('aria-busy')).toBe('true')
  await act(async () => { reject(new Error('db unavailable')) })
  expect(el.querySelector('[role="alert"]')).not.toBeNull()
  expect(el.querySelector('[role="alertdialog"]')).not.toBeNull()
})
it('returns focus after a successful persisted change', async () => {
  document.body.appendChild(el)
  root = createRoot(el)
  const change = vi.fn().mockResolvedValue(undefined)
  await act(async () => { root.render(<ApprovalModeSelector value="restricted" onChange={change} />) })
  const trigger = el.querySelector('button')!
  await act(async () => { trigger.click() })
  await act(async () => { el.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')[1].click() })
  expect(el.querySelector('[role="menu"]')).toBeNull()
  expect(document.activeElement).toBe(trigger)
})

it('requires explicit consent on every transition, defaults to cancel and blocks background shortcuts', async () => {
  document.body.appendChild(el); root = createRoot(el)
  const change = vi.fn().mockResolvedValue(undefined)
  await act(async () => root.render(<ApprovalModeSelector value="assisted" onChange={change} />))
  const trigger = el.querySelector('button')!
  const openBroad = async () => {
    await act(async () => trigger.click())
    await act(async () => el.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')[2].click())
  }
  await openBroad()
  await assertModalBlocksIntake()
  let buttons = el.querySelectorAll<HTMLButtonElement>('[role=alertdialog] button')
  expect(document.activeElement).toBe(buttons[0])
  expect(change).not.toHaveBeenCalled()
  const background = vi.fn()
  window.addEventListener('keydown', background)
  const event = new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true })
  await act(async () => buttons[0].dispatchEvent(event))
  window.removeEventListener('keydown', background)
  expect(event.defaultPrevented).toBe(true)
  expect(background).not.toHaveBeenCalled()
  await act(async () => buttons[0].click())
  expect(change).not.toHaveBeenCalled()
  expect(document.activeElement).toBe(trigger)
  await openBroad()
  buttons = el.querySelectorAll<HTMLButtonElement>('[role=alertdialog] button')
  await act(async () => buttons[1].click())
  expect(change).toHaveBeenCalledExactlyOnceWith('broad')
  expect(document.activeElement).toBe(trigger)
  await act(async () => root.render(<ApprovalModeSelector value="broad" onChange={change} />))
  expect(trigger.classList.contains('approval-mode-broad')).toBe(true)
  expect(el.querySelector('[role=alertdialog]')).toBeNull()
  await act(async () => trigger.click())
  await act(async () => el.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')[2].click())
  expect(el.querySelector('[role=alertdialog]')).toBeNull()
  await act(async () => root.render(<ApprovalModeSelector value="restricted" onChange={change} />))
  await openBroad()
  expect(el.querySelector('[role=alertdialog]')).not.toBeNull()
  await act(async () => el.querySelector('dialog')!.dispatchEvent(new Event('cancel', { cancelable: true })))
  expect(el.querySelector('[role=alertdialog]')).toBeNull()
  expect(change).toHaveBeenCalledTimes(1)
})

// Issue 10 / ADR 0051: confirmation controls use the live keymap; a remapped
// modal action is consumed here and cannot approve a background pending card.
it('honors remapped confirmation and cancel only while its dialog is open', async () => {
  document.body.appendChild(el); root = createRoot(el)
  const change = vi.fn().mockResolvedValue(undefined)
  setBinding('confirmBroadAccess', 'alt+shift+y')
  setBinding('cancelBroadAccess', 'alt+shift+n')
  try {
    await act(async () => root.render(<ApprovalModeSelector value="restricted" onChange={change} />))
    const open = async () => {
      await act(async () => el.querySelector('button')!.click())
      await act(async () => el.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')[2].click())
    }
    await open()
    expect(el.querySelectorAll('dialog button')[1].getAttribute('title')).toContain('Y')
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'N', altKey: true, shiftKey: true, cancelable: true })))
    expect(el.querySelector('dialog')).toBeNull()
    expect(change).not.toHaveBeenCalled()
    await open()
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Y', altKey: true, shiftKey: true, cancelable: true })))
    expect(change).toHaveBeenCalledExactlyOnceWith('broad')
  } finally { resetBinding('confirmBroadAccess'); resetBinding('cancelBroadAccess') }
})
