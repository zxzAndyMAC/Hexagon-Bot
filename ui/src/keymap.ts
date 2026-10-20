// 键位注册表（ADR 0051）：action id → 规范形 `mod+X`。
// mod 渲染按平台展开：macOS ⌘，其他 Ctrl。票 29 扩成可重映射全量表。

export type ActionId =
  | 'timelineLoadOlder'
  | 'timelineRetry'
  | 'timelineLatest'
  | 'timelineExport'
  | 'returnSummaryToggle'
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
  | 'desktopPrepareCapture'
  | 'computerModelSettings'
  | 'computerModelServices'
  | 'composerAttach'
  | 'sessionPause'
  | 'sessionReconnect'
  | 'sessionResume'
  | 'sessionContinueTrigger'
  | 'sessionCleanupWorktree'
  | 'sessionScheduleOnce'
  | 'sessionCancelTrigger'
  | 'sessionCreateWorktree'
  | 'sessionVerifyWorktree'

  | 'confirmBroadAccess'
  | 'cancelBroadAccess'
  | 'approvalMode'
  | 'managedEnable'
  | 'confirmManaged'
  | 'managedPause'
  | 'managedResume'
  | 'managedDisable'
  | 'managedReport'
  | 'managedExport'
  | 'modelRecoveryStop'
  | 'desktopPermissions'
  | 'desktopAccessibility'
  | 'desktopScreenRecording'
  | 'permissionOptions'
  | 'approve'
  | 'reject'
  | 'commandPalette'
  | 'toggleSidebar'
  | 'toggleCanvas'
  | 'canvasFullWidth'
  | 'focusMode'
  | 'closeTab'
  | 'closeOtherTabs'
  | 'closeTabsToRight'
  | 'copyMessage'
  | 'quoteMessage'
  | 'locateInRail'
  | 'mentionMember'
  | 'copyMemberName'
  | 'nextTab'
  | 'prevTab'
  | 'copyPath'
  | 'settings'
  | 'focusComposer'
  | 'stageRewind'
  | 'stageStamp'
  | 'nodeRail'
  | 'focusToasts'
  | 'stayInProject'
  | 'dismissPending'
  | 'treeNewFile'
  | 'treeNewFolder'
  | 'treeRefresh'
  | 'saveFile'
  | 'discardFileEdits'
  | 'cancelFileEdits'
  | 'selectionExplain'
  | 'selectionRewrite'
  | 'selectionSend'
  | 'copyArtifact'
  | 'confirmIntake'
  | 'reconcileAction'
  | 'abandonAction'
  | 'retryAction'
  | 'continueOriginalTask'
  | 'followNewInstruction'
  | 'retryWithChatModel'
  | 'rejudgeAfterModelFailure'
  | 'retryWizardGeneration'
  | 'stopWizardGeneration'
  | 'moveStageUp'
  | 'moveStageDown'
  | 'requestException'
  | 'reviewPerformanceBaseline'
  | 'reviewQualityCommands'
  | 'runQualityChecks'
  | 'confirmQualityRevalidation'
  | 'cancelQualityRevalidation'
  | 'confirmQualityCommands'
  | 'cancelQualityCommands'
  | 'confirmPerformanceBaseline'
  | 'cancelPerformanceBaseline'
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
  reviewQualityCommands: '', confirmQualityCommands: '', cancelQualityCommands: '',
  runQualityChecks: '',
  confirmQualityRevalidation: '', cancelQualityRevalidation: '',
  reviewPerformanceBaseline: '', confirmPerformanceBaseline: '', cancelPerformanceBaseline: '',
  timelineLoadOlder: '', timelineRetry: '', timelineLatest: '',
  // 票 05：导出轨迹、收起/展开归来摘要——默认无绑定，负责人可在设置·键盘指定
  timelineExport: '', returnSummaryToggle: '',
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
  desktopPrepareCapture: '',
  computerModelSettings: '',
  // ui-redesign 票 07：预检卡「打开模型服务」（control#readiness）与输入条「+ 附件」（D1）。
  // 默认无绑定，负责人可在设置·键盘指定；tooltip 跟随当前绑定。
  computerModelServices: '',
  composerAttach: '',
  sessionPause: 'alt+mod+shift+9',
  sessionReconnect: 'alt+mod+shift+0',
  sessionResume: 'alt+mod+shift+8',
  sessionContinueTrigger: 'alt+mod+shift+7',
  sessionCleanupWorktree: 'alt+mod+shift+6',
  sessionScheduleOnce: 'alt+mod+shift+5',
  // ui-redesign gap D1：取消「稍后继续」预约；默认无绑定，负责人可在设置·键盘指定。
  sessionCancelTrigger: '',
  sessionCreateWorktree: 'alt+mod+shift+w',
  sessionVerifyWorktree: 'alt+mod+shift+k',

  confirmBroadAccess: '',
  cancelBroadAccess: '',
  browserPanel: 'alt+mod+shift+b',
  browserManaged: '',
  browserConnect: '',
  browserInstallExtension: '',
  browserContinueConnection: '',
  browserDetach: '',
  approvalMode: 'alt+mod+shift+m',
  // 2026-10-11 UI alignment: no direct shortcut turns delegation on (the
  // explanation is mandatory); safe-direction actions are owner-assignable.
  managedEnable: '',
  confirmManaged: '',
  managedPause: '',
  managedResume: '',
  managedDisable: '',
  managedReport: '',
  managedExport: '',
  modelRecoveryStop: '',
  desktopPermissions: 'alt+mod+shift+d',
  desktopAccessibility: 'alt+mod+shift+a',
  desktopScreenRecording: 'alt+mod+shift+v',
  openRelatedExperience: 'alt+mod+shift+o',
  openExperienceSource: 'alt+mod+o',
  refreshExperienceHistory: 'alt+mod+shift+h',
  nextExperiencePage: 'alt+mod+ArrowDown',
  permissionOptions: '',
  approve: 'mod+Enter',
  reject: 'mod+Backspace',
  commandPalette: 'mod+K',
  // ADR 0088（ui-redesign 票 02）：⌘B 由「开关右栏」改为「开关左侧栏」，动作 id 随之改名；
  // 旧 id 的用户重映射由 migrateLegacyBindings 迁过来。⌥⌘B 开关画布（03 接入画布本体）。
  toggleSidebar: 'mod+B',
  toggleCanvas: 'alt+mod+B',
  // 票 03（R-04）：分屏随中栏 Tab 容器撤掉，⌘\ 改给画布全宽 ↔ 并排；旧 splitEditor 的重映射丢弃（DROPPED）。
  canvasFullWidth: 'mod+\\',
  // 票 03（R-06）：专注模式只留对话与输入条，再按恢复。
  focusMode: 'mod+.',
  // 票 03（ADR 0088）：⌘W 关画布当前页签（时间线不再是页签）。
  closeTab: 'mod+W',
  // owner-followups-1015 #05（A16b）：右键菜单的新动作默认不绑键位（alt+mod+shift+* 已挤满），
  // 进命令面板与设置·键盘，负责人自己指定。作用对象 = 当前页签（右键时 = 目标页签）。
  closeOtherTabs: '', closeTabsToRight: '', copyPath: '',
  // owner-followups-1015 #08（A16b 成员行 / 消息）：同上默认空绑定。消息动作作用于焦点所在的消息（或选区所在的消息），
  // 成员动作作用于焦点成员行；没有目标时什么也不做。
  copyMessage: '', quoteMessage: '', locateInRail: '', mentionMember: '', copyMemberName: '',
  // owner-followups-1015 #06（A22「溢出」）：页签多到滚出视区后键盘切换，同 VS Code / 浏览器的 ⌘⇧] / ⌘⇧[。
  nextTab: 'mod+shift+]', prevTab: 'mod+shift+[',
  settings: 'mod+,',
  focusComposer: 'mod+N',
  // ADR 0056-2：阶段操作键位（alt+mod 组合避开 ⌘ 系常用位）
  stageRewind: 'alt+mod+ArrowLeft',
  stageStamp: 'alt+mod+S',
  // ui-audit 票 12（P2-13）：节点轨键盘入口
  nodeRail: 'mod+J',
  // ui-redesign 票 13（A18 聚焦态）：F6 跳到通知区——toast 不抢焦点，键盘用户靠它够到「撤回」等动作。
  focusToasts: 'F6',
  // 票 13：切换项目阻塞横条「留在此项目」——默认空，可自设（同 timelineRetry）。
  stayInProject: '',
  // hands-free 票 05：收起待决弹窗。不用裸 Escape——确认层和命令面板
  // 已经占用它；mod+Escape 进键位表，关闭钮 tooltip 才能显示当前绑定。
  dismissPending: 'mod+Escape',
  // 票 11：文件树与中栏保存。alt+mod 避开 ⌘N（聚焦输入）与 ⌘R（浏览器刷新）。
  treeNewFile: 'alt+mod+n',
  treeNewFolder: 'alt+mod+f',
  treeRefresh: 'alt+mod+r',
  saveFile: 'mod+s',
  discardFileEdits: '',
  cancelFileEdits: 'Escape',
  // ui-redesign 票 09（D13 / D14）：画布选区浮条与产物复制。默认不绑定（只填输入条、不发送，没有抢键的必要），
  // 负责人可在设置·键盘指定；按钮 tooltip 读当前绑定。
  selectionExplain: '', selectionRewrite: '', selectionSend: '', copyArtifact: '',
  // 票 17：确认开场草案。mod+Enter 已是待决批准，加 shift 才不会误写 AGENTS.md。
  confirmIntake: 'mod+shift+Enter',
  reconcileAction: 'alt+mod+i',
  abandonAction: 'alt+mod+Backspace',
  retryAction: 'alt+mod+Enter',
  // MR-04: explicit owner choice for a held instruction; never `approve`.
  continueOriginalTask: 'alt+mod+c',
  followNewInstruction: 'alt+mod+u',
  // MR-05: explicit model switch for one request; `approve` never does this.
  retryWithChatModel: 'alt+mod+m',
  // MR-06: explicit re-judge after an internal judge request failed.
  rejudgeAfterModelFailure: 'alt+mod+j',
  // MR-07: wizard generation recovery on the current step.
  retryWizardGeneration: 'alt+mod+y',
  stopWizardGeneration: 'alt+mod+x',
  // ui-redesign 票 10（components E10 补设计）：流程草稿阶段行拖拽排序的键盘替代
  // （accessibility §3）。只在焦点落在阶段行内时生效，裸 alt+↑↓ 不占全局。
  moveStageUp: 'alt+ArrowUp',
  moveStageDown: 'alt+ArrowDown',
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
  { id: 'timelineExport', labelKey: 'timeline.export' },
  { id: 'returnSummaryToggle', labelKey: 'returnSummary.toggle' },
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
  { id: 'managedEnable', labelKey: 'delegation.turnOn' },
  { id: 'confirmManaged', labelKey: 'delegation.confirmAction' },
  { id: 'managedPause', labelKey: 'delegation.pause' },
  { id: 'managedResume', labelKey: 'delegation.resume' },
  { id: 'managedDisable', labelKey: 'delegation.turnOff' },
  { id: 'managedReport', labelKey: 'delegation.report' },
  { id: 'managedExport', labelKey: 'delegation.exportReport' },
  { id: 'modelRecoveryStop', labelKey: 'modelRecovery.stop' },
  { id: 'desktopPanel', labelKey: 'computer.title' },
  { id: 'desktopEnable', labelKey: 'computer.enable' },
  { id: 'desktopDisable', labelKey: 'computer.disable' },
  { id: 'desktopPause', labelKey: 'computer.pause' },
  { id: 'desktopResume', labelKey: 'computer.resume' },
  { id: 'reviewPerformanceBaseline', labelKey: 'quality.reviewBaseline' },
  { id: 'reviewQualityCommands', labelKey: 'quality.configReview' },
  { id: 'runQualityChecks', labelKey: 'quality.configRun' },
  { id: 'confirmQualityRevalidation', labelKey: 'quality.revalidationConfirm' },
  { id: 'cancelQualityRevalidation', labelKey: 'quality.revalidationCancel' },
  { id: 'confirmQualityCommands', labelKey: 'quality.configConfirm' },
  { id: 'cancelQualityCommands', labelKey: 'quality.configCancel' },
  { id: 'confirmPerformanceBaseline', labelKey: 'quality.confirmBaseline' },
  { id: 'cancelPerformanceBaseline', labelKey: 'quality.cancelBaseline' },
  { id: 'desktopRelease', labelKey: 'computer.release' },
  { id: 'desktopClear', labelKey: 'computer.clear_screenshots' },
  { id: 'desktopPrepareCapture', labelKey: 'computer.prepare_capture' },
  { id: 'computerModelSettings', labelKey: 'computer.modelSettings' },
  { id: 'computerModelServices', labelKey: 'computer.modelServices' },
  { id: 'composerAttach', labelKey: 'composer.attach' },
  { id: 'sessionPause', labelKey: 'session.pause' },
  { id: 'sessionReconnect', labelKey: 'session.reconnect' },
  { id: 'sessionResume', labelKey: 'session.resume' },
  { id: 'sessionContinueTrigger', labelKey: 'session.continue' },
  { id: 'sessionCleanupWorktree', labelKey: 'session.worktreeCleanup' },
  { id: 'sessionScheduleOnce', labelKey: 'session.scheduleLater' },
  { id: 'sessionCancelTrigger', labelKey: 'session.cancelBooking' },
  { id: 'sessionCreateWorktree', labelKey: 'session.worktreeCreate' },
  { id: 'sessionVerifyWorktree', labelKey: 'session.worktreeVerify' },

  { id: 'desktopPermissions', labelKey: 'desktop.check' },
  { id: 'desktopAccessibility', labelKey: 'desktop.openAccessibility' },
  { id: 'desktopScreenRecording', labelKey: 'desktop.openScreen' },
  { id: 'commandPalette', labelKey: 'keys.palette' },
  { id: 'toggleSidebar', labelKey: 'keys.sidebar' },
  { id: 'toggleCanvas', labelKey: 'keys.canvas' },
  { id: 'canvasFullWidth', labelKey: 'keys.canvasFullWidth' },
  { id: 'focusMode', labelKey: 'keys.focusMode' },
  { id: 'closeTab', labelKey: 'keys.closeTab' },
  { id: 'closeOtherTabs', labelKey: 'keys.closeOtherTabs' },
  { id: 'closeTabsToRight', labelKey: 'keys.closeTabsToRight' },
  { id: 'copyMessage', labelKey: 'keys.copyMessage' },
  { id: 'quoteMessage', labelKey: 'keys.quoteMessage' },
  { id: 'locateInRail', labelKey: 'keys.locateInRail' },
  { id: 'mentionMember', labelKey: 'keys.mentionMember' },
  { id: 'copyMemberName', labelKey: 'keys.copyMemberName' },
  { id: 'nextTab', labelKey: 'keys.nextTab' },
  { id: 'prevTab', labelKey: 'keys.prevTab' },
  { id: 'copyPath', labelKey: 'keys.copyPath' },
  { id: 'settings', labelKey: 'keys.settings' },
  { id: 'focusComposer', labelKey: 'keys.composer' },
  { id: 'permissionOptions', labelKey: 'projectPermission.options' },
  { id: 'approve', labelKey: 'keys.approve' },
  { id: 'reject', labelKey: 'keys.reject' },
  { id: 'stageRewind', labelKey: 'keys.stageRewind' },
  { id: 'stageStamp', labelKey: 'keys.stageStamp' },
  { id: 'nodeRail', labelKey: 'keys.nodeRail' },
  { id: 'focusToasts', labelKey: 'keys.focusToasts' },
  { id: 'stayInProject', labelKey: 'feedback.stayHere' },
  { id: 'dismissPending', labelKey: 'keys.dismissPending' },
  { id: 'treeNewFile', labelKey: 'keys.treeNewFile' },
  { id: 'treeNewFolder', labelKey: 'keys.treeNewFolder' },
  { id: 'treeRefresh', labelKey: 'keys.treeRefresh' },
  { id: 'saveFile', labelKey: 'keys.saveFile' },
  { id: 'discardFileEdits', labelKey: 'file.discard' },
  { id: 'cancelFileEdits', labelKey: 'agent.cancel' },
  { id: 'selectionExplain', labelKey: 'canvas.sel.explain' },
  { id: 'selectionRewrite', labelKey: 'canvas.sel.rewrite' },
  { id: 'selectionSend', labelKey: 'canvas.sel.toComposer' },
  { id: 'copyArtifact', labelKey: 'canvas.copy' },
  { id: 'confirmIntake', labelKey: 'keys.confirmIntake' },
  { id: 'reconcileAction', labelKey: 'cards.reconcileAction' },
  { id: 'abandonAction', labelKey: 'cards.abandonAction' },
  { id: 'retryAction', labelKey: 'cards.retryAction' },
  { id: 'continueOriginalTask', labelKey: 'modelRecovery.continueOriginal' },
  { id: 'followNewInstruction', labelKey: 'modelRecovery.followNew' },
  { id: 'retryWithChatModel', labelKey: 'modelRecovery.useChatModelAction' },
  { id: 'rejudgeAfterModelFailure', labelKey: 'judgeFailure.rejudge' },
  { id: 'retryWizardGeneration', labelKey: 'wizard.retryGeneration' },
  { id: 'stopWizardGeneration', labelKey: 'wizard.stopGeneration' },
  { id: 'moveStageUp', labelKey: 'wizard.moveUp' },
  { id: 'moveStageDown', labelKey: 'wizard.moveDown' },
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

/** 改名动作的旧 id → 新 id（ADR 0088：toggleRail → toggleSidebar）。 */
const RENAMED: Record<string, ActionId> = { toggleRail: 'toggleSidebar' }
/** 撤掉的动作（票 03，R-04：分屏随中栏 Tab 容器撤掉）：用户对它的重映射直接丢弃，不迁到新动作。 */
const DROPPED = ['splitEditor'] as const

/**
 * 把用户对已改名动作的重映射搬到新 id，删除旧键。新 id 已有用户绑定时以新的为准，旧键只清掉。
 * 模块加载时跑一次；幂等，没有旧键时什么也不做。
 */
export function migrateLegacyBindings() {
  for (const [from, to] of Object.entries(RENAMED)) {
    const old = localStorage.getItem(`hexagon.key.${from}`)
    if (old == null) continue
    if (localStorage.getItem(`hexagon.key.${to}`) == null) localStorage.setItem(`hexagon.key.${to}`, old)
    localStorage.removeItem(`hexagon.key.${from}`)
  }
  for (const gone of DROPPED) localStorage.removeItem(`hexagon.key.${gone}`)
}
migrateLegacyBindings()

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
  // owner-followups-1015 #06：Shift 会把 [ ] 改成 { }（美式布局），按 e.key 认则 mod+shift+] 永远不命中、
  // 设置·键盘录下的也是 mod+shift+}。方括号按物理键位还原，其余符号仍按字符认。
  if (e.shiftKey && (e.code === 'BracketLeft' || e.code === 'BracketRight')) return e.code === 'BracketLeft' ? '[' : ']'
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

/** `mod+Enter` → `⌘↵`（mac）/ `Ctrl+Enter`（其他） */
export function formatBinding(b: string): string {
  const parts = b.split('+')
  const key = parts[parts.length - 1]
  const mods = parts.slice(0, -1)
  const sym = (m: string) =>
    m === 'mod' ? (isMac ? '⌘' : 'Ctrl+') : m === 'shift' ? (isMac ? '⇧' : 'Shift+') : m === 'alt' ? (isMac ? '⌥' : 'Alt+') : m
  const keySym = { Enter: isMac ? '↵' : 'Enter', Backspace: isMac ? '⌫' : 'Backspace', Escape: 'Esc', '\\': '\\', ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→' }[key] ?? key.toUpperCase()
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
