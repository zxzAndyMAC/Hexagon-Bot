import { expect, it } from 'vitest'
import { ACTIONS, bindingFor } from './keymap'

it('never assigns the new computer controls an existing default action binding', () => {
  // 2026-10-01 integration: computer-panel/resume initially collided with
  // experience curation/rollback, allowing one key to operate two surfaces.
  const occupied = new Map<string, string>()
  for (const action of ACTIONS) {
    const binding = bindingFor(action.id).toLowerCase()
    if (!binding) continue
    // Owner extension 11 (2026-10-02): these native modal dialogs are mutually
    // exclusive and capture Escape before globals; sharing dismissal is safe.
    // I1: draft confirmation also captures dismissal; an open native dialog
    // takes precedence so Escape cannot discard a background draft prompt.
    if ((action.id === 'closeDesktopScreenshot' || action.id === 'cancelFileEdits') && binding === 'escape'
      && occupied.get(binding) === 'closeDesignPreview') continue
    expect(occupied.get(binding), `${action.id} conflicts at ${binding}`).toBeUndefined()
    occupied.set(binding, action.id)
  }
})
