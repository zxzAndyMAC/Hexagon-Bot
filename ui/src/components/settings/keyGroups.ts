// 设置「键盘」分区的动作分组（ui-redesign 票 11；settings#keys、components E7）。
// keymap.ts 的 ACTIONS 是扁平表，分区按 id 规则归组后自动列出——新增或改名的动作（票 02 toggleSidebar /
// toggleCanvas、票 03 canvasFullWidth / focusMode）不需要改这里也会出现；认不出的进「其他」，不会漏列。
import { ACTIONS, bindingFor, type ActionId } from '../../keymap'

export const KEY_GROUPS = ['decide', 'layout', 'managed', 'computer', 'session', 'files', 'other'] as const
export type KeyGroup = (typeof KEY_GROUPS)[number]

const DECIDE = new Set<string>([
  'approve', 'reject', 'dismissPending', 'permissionOptions', 'stageStamp', 'stageRewind', 'confirmIntake',
  'reconcileAction', 'abandonAction', 'retryAction', 'continueOriginalTask', 'followNewInstruction',
  'retryWithChatModel', 'rejudgeAfterModelFailure', 'requestException', 'acceptException',
])
const LAYOUT = new Set<string>([
  'commandPalette', 'settings', 'toggleSidebar', 'toggleCanvas', 'canvasFullWidth', 'focusMode',
  'closeTab', 'focusComposer', 'nodeRail',
])

/** 第一条命中的规则决定分组。 */
export function keyGroupOf(id: string): KeyGroup {
  if (DECIDE.has(id)) return 'decide'
  if (LAYOUT.has(id) || id.startsWith('timeline')) return 'layout'
  if (id.startsWith('managed') || id === 'confirmManaged' || id === 'approvalMode' || /BroadAccess$/.test(id) || id === 'modelRecoveryStop') return 'managed'
  if (/^(desktop|browser|agentScreen)/.test(id) || /(BrowserElement|DesktopScreenshot)$/.test(id) || id === 'computerModelSettings') return 'computer'
  if (id.startsWith('session')) return 'session'
  if (id.startsWith('tree') || /FileEdits?$/.test(id) || id === 'saveFile') return 'files'
  return 'other'
}

/** 按 KEY_GROUPS 顺序分组，组内保持 ACTIONS 原顺序；空组不出。 */
export function groupActions<T extends { id: ActionId | string }>(actions: readonly T[]): { group: KeyGroup; actions: T[] }[] {
  return KEY_GROUPS
    .map((group) => ({ group, actions: actions.filter((a) => keyGroupOf(a.id) === group) }))
    .filter((g) => g.actions.length > 0)
}

/** 系统保留组合：录制时拒绝写入（E7 禁用行「系统保留组合不可录」）。 */
export const RESERVED_BINDINGS = new Set(['mod+q', 'mod+h', 'mod+m', 'mod+tab', 'mod+space', 'mod+c', 'mod+v', 'mod+x', 'mod+z', 'mod+a'])

/** 规范形比较：修饰键集合相同、主键不分大小写（默认表写 `mod+K`，录制得到 `mod+k`）。 */
export function sameBinding(a: string, b: string): boolean {
  if (!a || !b) return false
  const norm = (x: string) => {
    const parts = x.split('+')
    const key = parts.pop()!.toLowerCase()
    return [...parts.map((m) => m.toLowerCase()).sort(), key].join('+')
  }
  return norm(a) === norm(b)
}

/**
 * 录制冲突：b 已被别的动作占用则返回那个动作。
 * 修复即文档（票 11）：keymap.conflictFor 用 `===` 比较，默认表的大写主键（`mod+K`）与录制得到的小写（`mod+k`）
 * 永远不相等——把 ⌘K 录给别的动作时冲突提示不出现，两个动作共用一个键。这里按规范形比较。
 */
export function findConflict(id: ActionId, b: string): ActionId | null {
  for (const a of ACTIONS) if (a.id !== id && sameBinding(bindingFor(a.id), b)) return a.id
  return null
}
