import { act } from 'react'
import { expect, vi } from 'vitest'
import { bindingFor, isMac, matches, resetBinding, setBinding } from './keymap'

/** Review regression: assert real modal capture stops App's intake shortcut,
 * including remapping; a native dialog alone does not stop a window listener. */
export async function assertModalBlocksIntake() {
  const confirm = vi.fn()
  const background = (event: KeyboardEvent) => {
    if (matches(event, bindingFor('confirmIntake'))) confirm()
  }
  window.addEventListener('keydown', background)
  try {
    const standard = new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, ctrlKey: !isMac, metaKey: isMac, bubbles: true, cancelable: true })
    await act(async () => window.dispatchEvent(standard))
    expect(standard.defaultPrevented).toBe(true)
    setBinding('confirmIntake', 'alt+shift+9')
    const remapped = new KeyboardEvent('keydown', { key: '9', altKey: true, shiftKey: true, bubbles: true, cancelable: true })
    await act(async () => window.dispatchEvent(remapped))
    expect(remapped.defaultPrevented).toBe(true)
    expect(confirm).not.toHaveBeenCalled()
  } finally {
    resetBinding('confirmIntake')
    window.removeEventListener('keydown', background)
  }
}
