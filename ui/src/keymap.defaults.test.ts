import { expect, it } from 'vitest'
import { ACTIONS, bindingFor } from './keymap'
import zh from './i18n/locales/zh-CN'

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

it('labels decision and settings actions with the prototype wording (ui-redesign gap D7)', () => {
  // Gap D7 (ticket 16B deviation 9, 2026-10-16): settings#keys / composer#palette
  // name these actions 「批准顶部待决卡」「拒绝顶部待决卡」「打开设置」 and the
  // practice area 「练习场」. Ids stay; only the i18n labels changed, so the
  // command palette and tooltips (which render labelKey) follow automatically.
  const label = (id: string) => ACTIONS.find(a => a.id === id)!.labelKey.replace(/^keys\./, '')
  const keys = zh.keys as Record<string, string>
  expect(keys[label('approve')]).toBe('批准顶部待决卡')
  expect(keys[label('reject')]).toBe('拒绝顶部待决卡')
  expect(keys[label('settings')]).toBe('打开设置')
  expect(keys[label('dismissPending')]).toBe('收起待决坞')
  expect(zh.settings.practiceTitle).toBe('练习场')
})
