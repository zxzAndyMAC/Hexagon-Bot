// 键位注册表（ADR 0051）：action id → 规范形 `mod+X`。
// mod 渲染按平台展开：macOS ⌘，其他 Ctrl。票 29 扩成可重映射全量表。

export type ActionId =
  | 'timelineLoadOlder'
  | 'timelineRetry'
  | 'timelineLatest'
  | 'browserPanel'
  | 'browserManaged'
  | 'browserConnect'
  | 'browserInstallExtension'
  | 'browserContinueConnection'
  | 'browserDetach'
  | 'selectBrowserElement'
  | 'removeBrowserElement'
  | 'viewBrowserElement'
  | 'closeBrowserElement'
  | 'agentScreen'
  | 'agentScreenExpand'
  | 'agentScreenHide'
  | 'agentScreenFocus'
  | 'agentScreenMove'
  | 'agentScreenSource'
  | 'previewDesignMockup'
  | 'closeDesignPreview'
  | 'chooseDesignDirection'
  | 'closeDesktopScreenshot'
  | 'retrySkillsLoad'
  | 'viewDesktopScreenshot'
  | 'desktopPanel'
  | 'desktopEnable'
  | 'desktopDisable'
  | 'desktopPause'
  | 'desktopResume'
  | 'desktopRelease'
  | 'desktopClear'

  | 'confirmBroadAccess'
  | 'cancelBroadAccess'
  | 'approvalMode'
  | 'desktopPermissions'
  | 'desktopAccessibility'
  | 'desktopScreenRecording'
  | 'approve'
  | 'reject'
  | 'commandPalette'
  | 'toggleRail'
  | 'splitEditor'
  | 'closeTab'
  | 'settings'
  | 'focusComposer'
  | 'stageRewind'
  | 'stageStamp'
  | 'nodeRail'
  | 'dismissPending'
  | 'treeNewFile'
  | 'treeNewFolder'
  | 'treeRefresh'
  | 'saveFile'
  | 'confirmIntake'
  | 'reconcileAction'
  | 'abandonAction'
  | 'retryAction'
  | 'requestException'
  | 'acceptException'
  | 'curateLegacyExperience'
  | 'rollbackExperience'
  | 'revokeExperience'
  | 'editProjectSkill'
  | 'saveProjectSkill'
  | 'saveExperienceLimits'
  | 'recoverExperience'
  | 'nextExperiencePage'
  | 'refreshExperienceHistory'
  | 'openExperienceSource'
  | 'openRelatedExperience'
  | 'openPolicyReport'

const DEFAULTS: Record<ActionId, string> = {
  timelineLoadOlder: '', timelineRetry: '', timelineLatest: '',
  selectBrowserElement: 'alt+mod+shift+l',
  removeBrowserElement: '',
  viewBrowserElement: '',
  closeBrowserElement: '',
  agentScreen: 'alt+mod+shift+f',
  agentScreenExpand: '',
  agentScreenHide: '',
  agentScreenFocus: '',
  agentScreenMove: '',
  agentScreenSource: '',
  previewDesignMockup: '',
  closeDesignPreview: 'Escape',
  chooseDesignDirection: '',
  closeDesktopScreenshot: 'Escape',
  retrySkillsLoad: '',
  viewDesktopScreenshot: '',
  desktopPanel: 'alt+mod+shift+u',
  desktopEnable: 'alt+mod+shift+1',
  desktopDisable: 'alt+mod+shift+2',
  desktopPause: 'alt+mod+shift+p',
  desktopResume: 'alt+mod+shift+j',
  desktopRelease: 'alt+mod+shift+3',
  desktopClear: 'alt+mod+shift+4',

  confirmBroadAccess: '',
  cancelBroadAccess: '',
  browserPanel: 'alt+mod+shift+b',
  browserManaged: '',
  browserConnect: '',
  browserInstallExtension: '',
  browserContinueConnection: '',
  browserDetach: '',
  approvalMode: 'alt+mod+shift+m',
  desktopPermissions: 'alt+mod+shift+d',
  desktopAccessibility: 'alt+mod+shift+a',
  desktopScreenRecording: 'alt+mod+shift+v',
  openRelatedExperience: 'alt+mod+shift+o',
  openExperienceSource: 'alt+mod+o',
  refreshExperienceHistory: 'alt+mod+shift+h',
  nextExperiencePage: 'alt+mod+ArrowDown',
  approve: 'mod+Enter',
  reject: 'mod+Backspace',
  commandPalette: 'mod+K',
  toggleRail: 'mod+B',
  splitEditor: 'mod+\\',
  closeTab: 'mod+W',
  settings: 'mod+,',
  focusComposer: 'mod+N',
  // ADR 0056-2：阶段操作键位（alt+mod 组合避开 ⌘ 系常用位）
  stageRewind: 'alt+mod+ArrowLeft',
  stageStamp: 'alt+mod+S',
  // ui-audit 票 12（P2-13）：节点轨键盘入口
  nodeRail: 'mod+J',
  // hands-free 票 05：收起待决弹窗。不用裸 Escape——确认层和命令面板
  // 已经占用它；mod+Escape 进键位表，关闭钮 tooltip 才能显示当前绑定。
  dismissPending: 'mod+Escape',
  // 票 11：文件树与中栏保存。alt+mod 避开 ⌘N（聚焦输入）与 ⌘R（浏览器刷新）。
  treeNewFile: 'alt+mod+n',
  treeNewFolder: 'alt+mod+f',
  treeRefresh: 'alt+mod+r',
  saveFile: 'mod+s',
  // 票 17：确认开场草案。mod+Enter 已是待决批准，加 shift 才不会误写 AGENTS.md。
  confirmIntake: 'mod+shift+Enter',
  reconcileAction: 'alt+mod+i',
  abandonAction: 'alt+mod+Backspace',
  retryAction: 'alt+mod+Enter',
  requestException: 'alt+mod+e',
  acceptException: 'alt+mod+shift+Enter',
  openPolicyReport: 'alt+mod+p',
  recoverExperience: 'alt+mod+g',
  saveExperienceLimits: 'alt+mod+l',
  editProjectSkill: 'alt+mod+shift+e',
  revokeExperience: 'alt+mod+shift+Backspace',
  rollbackExperience: 'alt+mod+shift+r',
  curateLegacyExperience: 'alt+mod+shift+c',
  saveProjectSkill: 'alt+mod+shift+s',
}

// navigator.platform 已弃用（MDN，ui-audit 票 11）：优先 userAgentData；
// 无该字段的浏览器退到 UA 嗅探。仅用于 ⌘/Ctrl 展示与红绿灯让位——
// 判错的代价是视觉错位，不是功能失效，容错优先。
const uaData = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData
export const isMac = uaData ? uaData.platform === 'macOS' : /Mac|iPhone|iPad/.test(navigator.userAgent)

/** 设置页「键盘」分区的全量动作表（顺序 = 展示顺序） */
export const ACTIONS: { id: ActionId; labelKey: string }[] = [
  { id: 'timelineLoadOlder', labelKey: 'timeline.loadOlder' },
  { id: 'timelineRetry', labelKey: 'timeline.retry' },
  { id: 'timelineLatest', labelKey: 'timeline.toBottom' },
  { id: 'selectBrowserElement', labelKey: 'elementContext.select' },
  { id: 'removeBrowserElement', labelKey: 'elementContext.remove' },
  { id: 'viewBrowserElement', labelKey: 'elementContext.view' },
  { id: 'closeBrowserElement', labelKey: 'elementContext.close' },
  { id: 'agentScreen', labelKey: 'agentScreen.title' },
  { id: 'agentScreenExpand', labelKey: 'agentScreen.expand' },
  { id: 'agentScreenHide', labelKey: 'agentScreen.hide' },
  { id: 'agentScreenFocus', labelKey: 'agentScreen.open' },
  { id: 'agentScreenMove', labelKey: 'agentScreen.move' },
  { id: 'agentScreenSource', labelKey: 'agentScreen.source' },
  { id: 'previewDesignMockup', labelKey: 'design.preview' },
  { id: 'closeDesignPreview', labelKey: 'design.closePreview' },
  { id: 'chooseDesignDirection', labelKey: 'design.choose' },
  { id: 'closeDesktopScreenshot', labelKey: 'computer.closeScreenshot' },
  { id: 'retrySkillsLoad', labelKey: 'skills.retryLoad' },
  { id: 'viewDesktopScreenshot', labelKey: 'computer.viewScreenshot' },
  { id: 'confirmBroadAccess', labelKey: 'approvalMode.confirmAction' },
  { id: 'cancelBroadAccess', labelKey: 'agent.cancel' },
  { id: 'browserPanel', labelKey: 'browser.title' },
  { id: 'browserManaged', labelKey: 'browser.managed' },
  { id: 'browserConnect', labelKey: 'browser.extension' },
  { id: 'browserInstallExtension', labelKey: 'browser.installExtension' },
  { id: 'browserContinueConnection', labelKey: 'browser.continueConnection' },
  { id: 'browserDetach', labelKey: 'browser.detach' },
  { id: 'approvalMode', labelKey: 'approvalMode.label' },
  { id: 'desktopPanel', labelKey: 'computer.title' },
  { id: 'desktopEnable', labelKey: 'computer.enable' },
  { id: 'desktopDisable', labelKey: 'computer.disable' },
  { id: 'desktopPause', labelKey: 'computer.pause' },
  { id: 'desktopResume', labelKey: 'computer.resume' },
  { id: 'desktopRelease', labelKey: 'computer.release' },
  { id: 'desktopClear', labelKey: 'computer.clear_screenshots' },

  { id: 'desktopPermissions', labelKey: 'desktop.check' },
  { id: 'desktopAccessibility', labelKey: 'desktop.openAccessibility' },
  { id: 'desktopScreenRecording', labelKey: 'desktop.openScreen' },
  { id: 'commandPalette', labelKey: 'keys.palette' },
  { id: 'toggleRail', labelKey: 'keys.rail' },
  { id: 'splitEditor', labelKey: 'keys.split' },
  { id: 'closeTab', labelKey: 'keys.closeTab' },
  { id: 'settings', labelKey: 'keys.settings' },
  { id: 'focusComposer', labelKey: 'keys.composer' },
  { id: 'approve', labelKey: 'keys.approve' },
  { id: 'reject', labelKey: 'keys.reject' },
  { id: 'stageRewind', labelKey: 'keys.stageRewind' },
  { id: 'stageStamp', labelKey: 'keys.stageStamp' },
  { id: 'nodeRail', labelKey: 'keys.nodeRail' },
  { id: 'dismissPending', labelKey: 'keys.dismissPending' },
  { id: 'treeNewFile', labelKey: 'keys.treeNewFile' },
  { id: 'treeNewFolder', labelKey: 'keys.treeNewFolder' },
  { id: 'treeRefresh', labelKey: 'keys.treeRefresh' },
  { id: 'saveFile', labelKey: 'keys.saveFile' },
  { id: 'confirmIntake', labelKey: 'keys.confirmIntake' },
  { id: 'reconcileAction', labelKey: 'cards.reconcileAction' },
  { id: 'abandonAction', labelKey: 'cards.abandonAction' },
  { id: 'retryAction', labelKey: 'cards.retryAction' },
  { id: 'requestException', labelKey: 'exceptions.request' },
  { id: 'acceptException', labelKey: 'exceptions.accept' },
  { id: 'curateLegacyExperience', labelKey: 'experience.curate' },
  { id: 'rollbackExperience', labelKey: 'experience.withdraw' },
  { id: 'revokeExperience', labelKey: 'experience.revoke' },
  { id: 'editProjectSkill', labelKey: 'skills.edit' },
  { id: 'saveProjectSkill', labelKey: 'skills.save' },
  { id: 'saveExperienceLimits', labelKey: 'experience.saveLimits' },
  { id: 'nextExperiencePage', labelKey: 'experience.nextPage' },
  { id: 'refreshExperienceHistory', labelKey: 'experience.refreshHistory' },
  { id: 'openExperienceSource', labelKey: 'experience.openSource' },
  { id: 'openRelatedExperience', labelKey: 'experience.openRelated' },
  { id: 'recoverExperience', labelKey: 'experience.recover' },
  { id: 'openPolicyReport', labelKey: 'policy.report' },
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

function shortcutKey(e: KeyboardEvent | React.KeyboardEvent): string {
  // 2026-10-01 Settings pause acceptance: macOS Option changes key (P → ∏),
  // so character-only matching silently disabled the emergency shortcut.
  // Resolve only Command+Option letter/digit codes; matches still checks every
  // modifier exactly (especially Ctrl vs Command) before accepting an action.
  if (isMac && e.metaKey && e.altKey && /^(Key[A-Z]|Digit[0-9])$/.test(e.code)) {
    return e.code.replace(/^(Key|Digit)/, '').toLowerCase()
  }
  return e.key === ' ' ? 'space' : e.key.length === 1 ? e.key.toLowerCase() : e.key
}

/** KeyboardEvent → 规范形 `mod+x`；纯修饰键返回 null（继续等下一个键） */
export function normalizeEvent(e: KeyboardEvent | React.KeyboardEvent): string | null {
  if (['Meta', 'Control', 'Shift', 'Alt'].includes(e.key)) return null
  const mods: string[] = []
  if (e.metaKey || e.ctrlKey) mods.push('mod')
  if (e.altKey) mods.push('alt')
  if (e.shiftKey) mods.push('shift')
  const key = shortcutKey(e)
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
  const keySym = { Enter: isMac ? '↵' : 'Enter', Backspace: isMac ? '⌫' : 'Backspace', Escape: 'Esc', '\\': '\\' }[key] ?? key.toUpperCase()
  return mods.map(sym).join('') + keySym
}

/** 键盘事件是否命中绑定 */
export function matches(e: KeyboardEvent | React.KeyboardEvent, binding: string): boolean {
  const parts = binding.split('+')
  const key = parts[parts.length - 1].toLowerCase()
  const wantMod = parts.includes('mod')
  const wantShift = parts.includes('shift')
  const wantAlt = parts.includes('alt')
  // ui-audit 票 15（P3）：mod 平台精确化——macOS 的 mod 只认 metaKey，
  // 其他平台只认 ctrlKey，且另一把修饰键不得同时按下。
  // 旧实现 meta||ctrl 合并：mac 上 Ctrl+↵ 会误触「批准待决」——
  // 裁决键必须精确，宁漏不误（fail-closed：漏触发花一次鼠标，误触发花一次未审副作用）。
  const modHit = isMac ? e.metaKey : e.ctrlKey
  const otherMod = isMac ? e.ctrlKey : e.metaKey
  if (modHit !== wantMod || otherMod) return false
  if (e.shiftKey !== wantShift) return false
  if (e.altKey !== wantAlt) return false
  const evKey = shortcutKey(e).toLowerCase()
  return evKey === key.toLowerCase()
}
