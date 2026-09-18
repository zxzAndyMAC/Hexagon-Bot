// 键位注册表（ADR 0051）：action id → 规范形 `mod+X`。
// mod 渲染按平台展开：macOS ⌘，其他 Ctrl。票 29 扩成可重映射全量表。

export type ActionId =
  | 'approve'
  | 'reject'
  | 'commandPalette'
  | 'toggleRail'
  | 'splitEditor'
  | 'closeTab'
  | 'settings'
  | 'focusComposer'

const DEFAULTS: Record<ActionId, string> = {
  approve: 'mod+Enter',
  reject: 'mod+Backspace',
  commandPalette: 'mod+K',
  toggleRail: 'mod+B',
  splitEditor: 'mod+\\',
  closeTab: 'mod+W',
  settings: 'mod+,',
  focusComposer: 'mod+N',
}

export const isMac = navigator.platform.toUpperCase().includes('MAC')

/** 设置页「键盘」分区的全量动作表（顺序 = 展示顺序） */
export const ACTIONS: { id: ActionId; labelKey: string }[] = [
  { id: 'commandPalette', labelKey: 'keys.palette' },
  { id: 'toggleRail', labelKey: 'keys.rail' },
  { id: 'splitEditor', labelKey: 'keys.split' },
  { id: 'closeTab', labelKey: 'keys.closeTab' },
  { id: 'settings', labelKey: 'keys.settings' },
  { id: 'focusComposer', labelKey: 'keys.composer' },
  { id: 'approve', labelKey: 'keys.approve' },
  { id: 'reject', labelKey: 'keys.reject' },
]

export function bindingFor(a: ActionId): string {
  return localStorage.getItem(`hexagon.key.${a}`) ?? DEFAULTS[a]
}

export function setBinding(a: ActionId, b: string) {
  localStorage.setItem(`hexagon.key.${a}`, b)
}

export function resetBinding(a: ActionId) {
  localStorage.removeItem(`hexagon.key.${a}`)
}

/** KeyboardEvent → 规范形 `mod+x`；纯修饰键返回 null（继续等下一个键） */
export function normalizeEvent(e: KeyboardEvent | React.KeyboardEvent): string | null {
  if (['Meta', 'Control', 'Shift', 'Alt'].includes(e.key)) return null
  const mods: string[] = []
  if (e.metaKey || e.ctrlKey) mods.push('mod')
  if (e.altKey) mods.push('alt')
  if (e.shiftKey) mods.push('shift')
  const key = e.key === ' ' ? 'space' : e.key.length === 1 ? e.key.toLowerCase() : e.key
  return [...mods, key].join('+')
}

/** 绑定冲突检测：b 已被别的 action 占用则返回那个 action id */
export function conflictFor(a: ActionId, b: string): ActionId | null {
  for (const { id } of ACTIONS) {
    if (id !== a && bindingFor(id) === b) return id
  }
  return null
}

/** `mod+Enter` → `⌘↵`（mac）/ `Ctrl+Enter`（其他） */
export function formatBinding(b: string): string {
  const parts = b.split('+')
  const key = parts[parts.length - 1]
  const mods = parts.slice(0, -1)
  const sym = (m: string) =>
    m === 'mod' ? (isMac ? '⌘' : 'Ctrl+') : m === 'shift' ? (isMac ? '⇧' : 'Shift+') : m === 'alt' ? (isMac ? '⌥' : 'Alt+') : m
  const keySym = { Enter: isMac ? '↵' : 'Enter', Backspace: isMac ? '⌫' : 'Backspace', '\\': '\\' }[key] ?? key.toUpperCase()
  return mods.map(sym).join('') + keySym
}

/** 键盘事件是否命中绑定 */
export function matches(e: KeyboardEvent | React.KeyboardEvent, binding: string): boolean {
  const parts = binding.split('+')
  const key = parts[parts.length - 1].toLowerCase()
  const wantMod = parts.includes('mod')
  const wantShift = parts.includes('shift')
  const wantAlt = parts.includes('alt')
  if ((e.metaKey || e.ctrlKey) !== wantMod) return false
  if (e.shiftKey !== wantShift) return false
  if (e.altKey !== wantAlt) return false
  const evKey = e.key === ' ' ? 'space' : e.key.toLowerCase()
  return evKey === key.toLowerCase()
}
