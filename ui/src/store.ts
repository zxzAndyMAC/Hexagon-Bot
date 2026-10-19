import type { TimelineFacts } from './gen/TimelineFacts'
import type { ProjectIdentity } from './gen/ProjectIdentity'
import type { TimelineToolStreamKey } from './gen/TimelineToolStreamKey'
import { watchedTimelineMessages } from './useTimelineWindow'
import { create } from 'zustand'
import {
  api,
  errText,
  type ArtifactRow,
  type PendingQuestion,
  type ProposalRow,
  type StageRow,
  type ProvidersView,
  type TeamRow,
  type TimelineItem,
  type TurnDelta,
  type ToolOutputDelta,
  type ContextPressure,
  type UsageBucket,
  type UsageRow,
  type UsageTotal,
  type SessionSummary,
} from './api'
import { daysAgo } from './usage'
import { readCanvasLayout, readSideLayout, writeCanvasLayout, writeSideLayout, type CanvasLayout, type SideLayout } from './layout'
import i18n from './i18n'

import { themeSlice, type ThemeSlice } from './storeTheme'
export type { ThemePref } from './storeTheme'

/// 慢通道切片（arch-review 票 07）：invalidate 的失效标签。
/// usage=账本；artifacts=产物表；team=花名册+头像；info=项目元信息+autonomy。
export type SlowSlice = 'usage' | 'artifacts' | 'team' | 'info'

// 画布页签（ui-redesign 票 03，ADR 0088；components B7）：中栏 Tab 容器撤掉，时间线不再是页签（主区常驻），
// 用量详情不进画布（主区视图，interaction §10）。id 规则沿用旧 WorkTab。
// `artifacts` / `files` 是侧栏「产物」「项目文件」打开的清单页签（id 同名，各至多一个）。
export type TabKind = 'artifact' | 'diff' | 'agent' | 'file' | 'artifacts' | 'files'
/// 侧栏「产物」「项目文件」在画布里打开的清单页签。
export type CanvasPane = 'artifacts' | 'files'
export type FileTreeAction = 'new-file' | 'new-folder' | 'refresh'

/// 界面作用域（ui-audit 票 01）：全局快捷键分发按它裁决。
/// 非 workbench 时裁决类快捷键不得触发——设置页/命令面板打开期间，
/// 键盘批准会作用于看不见的待决卡（P0-1）。
export type ModalScope = 'workbench' | 'settings' | 'palette' | 'flow'

/// toast 通知（ui-audit 票 01/04）：用户发起变更的失败出口。
/// 轮询类错误不得走这里刷屏（调用方节流）。
/// ui-redesign 票 13（A18 Sonner 形制）：队列仍在 store（测试与调用方读它），
/// 寿命归 ToastHost/Sonner——约 3.5s、悬停暂停；Sonner 关闭时回调 dismissToast。
/// 曾经的写法：store 自己 setTimeout 3.5s 删项——悬停暂停无从实现（计时不在渲染层）。
export interface ToastAction {
  label: string
  run: () => unknown | Promise<unknown>
}
export interface ToastOpts {
  /** 原因码（C5「<原因>（<码>）」）：只放已登记的领域码，如 worktree_binding_drift（D8）。 */
  code?: string
  /** 一个文字动作（「撤回」「从最近项目移除」）。 */
  action?: ToastAction
  /** 前置图标：默认按 tone（ok=circleCheck / err=circleX）；undo = 撤销类结果。 */
  icon?: 'undo'
}
export interface Toast extends ToastOpts {
  id: number
  text: string
  tone: 'info' | 'ok' | 'err'
}
let toastSeq = 0

// ui-audit 票 04（P1-6）：轮询类失败的噪音节流——快通道 2s 一拍，
// 断网时裸 Promise.all 会每拍抛一次 unhandled rejection 且用户无感知。
// 连续失败到第 3 拍才 toast 一次；恢复即复位，不再刷屏。
let fastFailStreak = 0
const FAST_FAIL_TOAST_AT = 3

// 票 08 交接兜底：持久消息正常在一拍（2s）内到达；10s 未等到说明
// 该回合没有落库消息（turn_failed/中断等）——清缓冲防泄漏。
export const STREAM_HANDOFF_MS = 10_000

// exec-cards 票 04：toolStreams 单键尾留上限——background 任务比调用活得久，
// 缓冲不能随任务寿命无限长；渲染层另截末 32KB（ExecCard.TAIL_CAP）。
const TOOL_STREAM_CAP = 128 * 1024

// ui-audit 票 06（P2-9）：项目切换代际守卫。refresh() 递增代际；
// refreshFast/refreshSlow 起飞时捕获、落地前比对——切项目瞬间在飞的
// 旧响应被整体丢弃，旧项目事件不再缝进新时间线/新状态。
let generation = 0
let fastRequestSeq = 0
let lastAppliedFastRequest = 0

/// 确认层请求（ui-audit 票 03，ADR 0056）：L2 流程破坏性操作
/// （rewind/skip、⌘K 危险项）统一走应用内确认层，替代原生 confirm()。
/// 分级：L1 可逆直执行；L2 流程破坏走确认层；L3 对外不可逆走卡内 danger+禁键。
export interface ConfirmReq {
  title: string
  body?: string
  danger?: boolean
  confirmLabel?: string
  /** 带输入的确认（理由/名称等）：渲染输入框，值作 run 参数；
      required 时空值禁确认（ui-audit-2 票 08：override/驳回理由走此） */
  input?: { placeholder?: string; required?: boolean }
  run: (inputValue?: string) => unknown | Promise<unknown>
  confirmAction?: import('./keymap').ActionId
  cancelAction?: import('./keymap').ActionId
  alternate?: { label: string; action?: import('./keymap').ActionId; run: () => unknown | Promise<unknown> }
  skipRefresh?: boolean
  onClose?: (confirmed: boolean) => void
  /** 'leave' = 离开未保存（D17 LeaveGuard 形制：放弃修改在左、取消、保存 ⌘S）。票 09。 */
  variant?: 'leave'
}

export interface FileEdit {
  path: string
  dirty: boolean
  save: () => Promise<boolean>
  /** 「放弃修改」：缓冲回到已保存文本、清脏标记（票 09：进设置时编辑器不卸载，只关守卫不回填会留着脏稿）。 */
  discard?: () => void
}

export interface WorkTab {
  id: string // art:<path> | diff:<path>:<a>-<b> | agent:<id> | patch:<proposalId> | file:<path> | artifacts | files
  kind: TabKind
  title: string
  path?: string
  version?: number
  agentId?: string
  role?: string
  diffFrom?: number
  diffTo?: number
  patchText?: string
}

/// 专注模式（⌘.，R-06）进入前的布局快照；只在内存，不跨重启。
export interface FocusSnapshot { canvasOpen: boolean }

/// 清单页签的标题渲染时走 i18n（sidebar.artifacts / sidebar.files）。
const paneTab = (pane: CanvasPane): WorkTab => ({ id: pane, kind: pane, title: '' })

interface UiState extends ThemeSlice {
  projectRoot: string | null
  projectGeneration: number | null
  projectEpoch: number
  beginProjectSwitch: () => void
  commitProjectIdentity: (identity: ProjectIdentity | null, epoch: number) => void
  timelineFacts: TimelineFacts | null
  factsError: string | null
  sessionSummary: SessionSummary | null
  sessionError: string | null
  toolStreamKeys: Record<string, TimelineToolStreamKey>
  refreshFacts: () => Promise<void>
  stages: StageRow[]
  team: TeamRow[]
  artifacts: ArtifactRow[]
  evidenceRevision: number
  timelineCaughtUp: boolean
  timeline: TimelineItem[]
  pending: PendingQuestion[]
  /* hands-free 票 05：待决不再占中栏。弹窗关掉 ≠ 驳回。
     dismissedPendingKeys 记下已经收起的 id（q: 待决卡 / r: in_review 提案）。
     只有没见过的 id 才再弹——漏弹人去点徽标，误弹只是烦；
     把收起当成销卡会把必须人处理的卡静默丢掉（spec 故事 52/53），所以偏向再弹。
     常驻区的 pendingH / pendingCollapsed 随区一起撤，高度还给中栏和时间线。 */
  reviewRows: ProposalRow[]
  /* ui-redesign 票 04（ADR 0089）：弹窗换成输入条上方的待决坞。字段名沿用旧名（侧栏、命令面板、
     证据卡都在调 openPendingDialog，改名只换来跨票冲突），语义平移：
     pendingDialogOpen = 坞展开（false 且有待决时是「N 件待决」胶囊，不是隐藏）；
     dismissedPendingKeys = 收起时已见过的 id，新 id 到达自动展开（偏向再弹的理由同上）。 */
  pendingDialogOpen: boolean
  dismissedPendingKeys: string[]
  /* 票 04「保存即消卡」（interaction §2）：点下裁决到 invalidate 读回之间，这张卡先从坞里拿掉。
     只在途中有效——读回后无论结果如何都清掉，卡还在 pending 里就回到坞（不臆造已解决）。 */
  hiddenPending: string[]
  /* 裁决失败（D6 错误态）：卡回到坞顶，底栏上方红字原因。下一次成功裁决或换项目时清掉。 */
  pendingError: { id: string; text: string } | null
  usageTotal: UsageTotal | null
  autonomy: string
  /// MCP 还在握手时为 true。工作台已进入，输入先停。
  mcpPending: boolean
  projectName: string
  mode: 'pack' | 'fastpath'
  fastRole: string | null
  packName: string | null
  /// 左侧栏宽度与收起（ADR 0088；`hexagon.layout.side`，旧 `hexagon.rail` 迁移一次，见 layout.ts）。
  sideLayout: SideLayout
  /// 画布开关（票 03，components B7）。不持久化——页签不跨重启，开着一个空画布没有意义。
  canvasOpen: boolean
  /// 画布宽度与全宽（`hexagon.layout.canvas`，interaction §1）。
  canvasLayout: CanvasLayout
  /// 用量详情占满主区（interaction §10）；与画布全宽一起构成主区视图三态（layout.ts mainView）。
  usageView: boolean
  /// 专注模式（⌘.，R-06）：非 null 即开启——侧栏隐藏、画布关闭、节点轨隐藏。
  focusSnapshot: FocusSnapshot | null
  /// Agent Screen 有目标时头部才出「屏幕」按钮（components B5 禁用行）；req 脉冲让浮窗切换显隐。
  agentScreenAvailable: boolean
  agentScreenReq: number
  /// 快通道（阶段 + 待决）最近一次读取失败（components B1 错误行，补设计）：
  /// 侧栏待决徽标此时不显示数字，侧栏底出「状态读取失败 · 重试」。下次读成功即清除。
  statusError: boolean
  usageRows: UsageRow[]
  /// 7 日 token 序列缓存（票 17 / 方向卡 2）：随 usage 慢切片一起拉，
  /// 顶栏 sparkline 悬停零新增 IPC。
  usageSeries7d: UsageBucket[]
  /// 撞限压力（context-window 票 03 / ADR 0068）：近 14 天撞限卡+压缩计数，
  /// 用量页度量闸——≥2 张撞限卡触发恢复层设计票。
  contextPressure: ContextPressure | null
  avatars: Record<string, string>
  /// 已解析的头像哈希簿（票 07）：agent → TeamRow.avatar_hash。
  /// team 行哈希变了才重拉 data URL；null=已确认无头像。
  avatarHashes: Record<string, string | null>
  /// 供应商+槽位文档视图（ui-audit-2 票 01）：随 team 切片拉取——
  /// 团队行/角色编辑器据此把槽位名翻译成「供应商+模型」标签。
  providers: ProvidersView | null
  /// 画布页签（票 03）。时间线不在其中。
  tabs: WorkTab[]
  /// 画布当前页签；null = 没有页签。
  activeTab: string | null
  modalScope: ModalScope
  flowDialogOpen: boolean
  toasts: Toast[]
  confirmReq: ConfirmReq | null
  fileEdits: Record<string, FileEdit>
  setFileEdit: (id: string, edit: FileEdit | null) => void
  confirmFileEdits: (ids?: string[]) => Promise<boolean>
  /// 流式增量缓冲（票 03）：agent → 调用序号 → 累计文本。
  /// 瞬时态——不落盘。
  streams: Record<string, Record<number, string>>
  /// 思考增量（hands-free 票 06）：与 streams 同键。空 = 模型没给推理，不编造。
  plans: Record<string, string>
  thinkings: Record<string, Record<number, string>>
  /// 流式交接簿（ui-audit 票 08 / P1-7）：done 到达不立即删气泡——
  /// 标 {afterEventId: 当时时间线末条 id, at: 时间戳}；等 refreshFast
  /// 拉到同 agent 的持久 agent_message（id > afterEventId）才清缓冲，
  /// 消除 done→持久化之间的内容空窗与排版跳变。超 STREAM_HANDOFF_MS
  /// 未等到（turn_failed 无消息等路径）兜底清除，防泄漏。
  streamDone: Record<string, { afterEventId: number; at: number; hydrating?: boolean }>
  /// bash 输出瞬时缓冲（exec-cards 票 04）：`${agentId}:${seq}` → 追加文本
  /// （stdout/stderr 按到达序混排）。瞬时态——tool_result 落地后卡体改渲
  /// result.output，缓冲即清。seq 缺席（resolve 等非回合路径）落 `·` 键。
  toolStreams: Record<string, string>
  /// 断网等网态（network-resilience 票 02）：agent_id → 进入等网时刻。
  /// waiting:true 帧置位；其后任何非等网帧（正常增量/复位/清旗/done）
  /// 都清除——等网只活在「下一次调用还没出声」的窗口里。
  waitingSince: Record<string, number>
  applyToolOutput: (d: ToolOutputDelta) => void
  setSideLayout: (next: SideLayout) => void
  toggleSidebar: () => void
  /// 侧栏「产物」并排打开，「项目文件」以全宽打开（canvas#file）。
  openCanvas: (pane: CanvasPane) => void
  setCanvasOpen: (v: boolean) => void
  /// ⌥⌘B 与头部画布开关：画布可见（开着且没被用量详情盖住）就关，否则打开。
  toggleCanvas: () => void
  setCanvasLayout: (next: CanvasLayout) => void
  /// ⌘\（`canvasFullWidth`，R-04）：全宽 ↔ 并排；画布关着时以全宽打开。
  toggleCanvasFull: () => void
  /// 回到对话：退出画布全宽与用量详情（⌘N、全宽时的「对话」页签）。
  showConversation: () => void
  openUsageDetail: () => void
  /// ⌘.（`focusMode`）：只留对话与输入条，再按恢复进入前的布局。
  toggleFocusMode: () => void
  setFlowDialogOpen: (v: boolean) => void
  requestAgentScreen: () => void
  openPendingDialog: () => void
  closePendingDialog: () => void
  noteReviewRows: (rows: ProposalRow[]) => void
  syncPendingDialog: () => void
  hidePending: (id: string) => void
  unhidePending: (id: string) => void
  notePendingError: (error: { id: string; text: string } | null) => void
  /// 右栏文件树命令（票 11）：快捷键与按钮共用。n 递增，FileTree 消费一次。
  fileTreeReq: { n: number; action: FileTreeAction } | null
  requestFileTree: (action: FileTreeAction) => void
  /// 中栏当前文件的保存脉冲（mod+S）。0 = 从未请求。
  saveFileReq: number
  requestSaveFile: () => void
  /// 时间线贴底脉冲。发送成功后递增；0 = 从未请求。失败不递增。
  timelineStickReq: number
  requestTimelineStick: () => void
  openTab: (t: WorkTab) => void
  closeTab: (id: string) => void
  /** 一次关多个页签（A16b「关闭其他 / 关闭右侧」）；未保存的文件合并成一次离开确认。 */
  closeTabs: (ids: string[]) => void
  setActiveTab: (id: string) => void
  setModalScope: (s: ModalScope) => void
  pushToast: (text: string, tone?: Toast['tone'], opts?: ToastOpts) => void
  dismissToast: (id: number) => void
  askConfirm: (r: ConfirmReq) => void
  clearConfirm: (confirmed?: boolean) => void
  /// 节点轨聚焦脉冲（票 12）：mod+J 递增 → NodeRail 展开+聚焦首条。
  nodeRailPulse: number
  focusNodeRail: () => void
  /// 值守态（票 16 / 方向卡 1）：owner_away/back 的 UI 侧开关。
  /// 手动 chip + 窗口 blur>60s/focus 自动；markBack 后 refreshFast 拉
  /// return_summary 行。失败走 pushToast（票 04 错误出口约定）。
  /// 票 17：开场分析附的项目说明草案还在等人点头。不是待决弹窗。
  intakeDraft: boolean
  away: boolean
  markAway: () => Promise<void>
  markBack: () => Promise<void>
  applyDelta: (d: TurnDelta) => void
  refresh: () => Promise<void>
  /// 快通道（arch-review 票 07）：2s 轮询面 = stages+pending+timeline 增量，
  /// 外加开场草案是否还在（票 17，失败不拖垮这一拍）。timeline 走 after 游标追加；空时间线=首拉全量。
  refreshFast: () => Promise<void>
  /// 慢通道：usage/artifacts/team(+avatars)/info，仅失效标签驱动，不轮询。
  /// tags 缺省 = 全切片。
  refreshSlow: (tags?: SlowSlice[]) => Promise<void>
  /// 组件写操作后统一入口（取代散点 await refresh()）：
  /// 快通道补一拍 + 按标签拉慢切片。`invalidate()` = 全失效。
  invalidate: (...tags: SlowSlice[]) => Promise<void>
}

function humanKeys(pending: PendingQuestion[], reviewRows: { id: string }[]): string[] {
  return [...pending.map((p) => `q:${p.id}`), ...reviewRows.map((r) => `r:${r.id}`)]
}

export const useUiStore = create<UiState>((set, get) => ({
  projectRoot: null,
  projectGeneration: null,
  projectEpoch: 0,
  timelineFacts: null,
  factsError: null,
  sessionSummary: null,
  sessionError: null,
  toolStreamKeys: {},
  beginProjectSwitch: () => {
    // Issue15/14: invalidate synchronously before the first project IPC awaits.
    // Same-name projects still have distinct epochs; failed switches never admit
    // old responses when restoring the original root.
    generation++
    const priorConfirmation = get().confirmReq
    set(s => ({ projectRoot: null, projectGeneration: null, projectEpoch: s.projectEpoch + 1,
      // Benchmark I1: p1 is reused across projects. Clear actionable projections
      // at the same identity boundary, before a status poll can fail or arrive late.
      stages: [], pending: [], reviewRows: [], pendingDialogOpen: false, dismissedPendingKeys: [], hiddenPending: [], pendingError: null, statusError: false,
      team: [], artifacts: [], avatars: {}, avatarHashes: {}, providers: null,
      usageTotal: null, usageRows: [], usageSeries7d: [], contextPressure: null,
      projectName: '', autonomy: 'L0', mode: 'pack', fastRole: null, packName: null, mcpPending: false,
      tabs: [], activeTab: null, canvasOpen: false, usageView: false, flowDialogOpen: false,
      fileEdits: {}, fileTreeReq: null, confirmReq: null, intakeDraft: false, away: false,
      evidenceRevision: s.evidenceRevision + 1,
      timelineFacts: null, factsError: null, sessionSummary: null, sessionError: null, timeline: [], timelineCaughtUp: false,
      streams: {}, thinkings: {}, streamDone: {}, toolStreams: {}, toolStreamKeys: {}, plans: {}, waitingSince: {} }))
    priorConfirmation?.onClose?.(false)
  },
  commitProjectIdentity: (identity, epoch) => { if (get().projectEpoch === epoch) set({ projectRoot: identity?.project_root ?? null, projectGeneration: identity?.generation ?? null }) },
  ...themeSlice(set, () => get().themePref),
  stages: [],
  team: [],
  artifacts: [],
  evidenceRevision: 0,
  timelineCaughtUp: false,
  timeline: [],
  pending: [],
  reviewRows: [],
  pendingDialogOpen: false,
  dismissedPendingKeys: [],
  hiddenPending: [],
  pendingError: null,
  usageTotal: null,
  autonomy: 'L0',
  // 票 15（P3）：未加载时留空串——TopBar 回退 t('app.untitledProject')，
  // 不再以 mock 项目名「食谱 App」示人（真 Tauri 下首帧会闪假名）。
  mcpPending: false,
  projectName: '',
  mode: 'pack',
  fastRole: null,
  packName: null,
  sideLayout: readSideLayout(localStorage),
  canvasOpen: false,
  canvasLayout: readCanvasLayout(localStorage),
  usageView: false,
  focusSnapshot: null,
  agentScreenAvailable: false,
  agentScreenReq: 0,
  statusError: false,
  fileTreeReq: null,
  saveFileReq: 0,
  timelineStickReq: 0,
  usageRows: [],
  usageSeries7d: [],
  contextPressure: null,
  avatars: {},
  avatarHashes: {},
  providers: null,
  tabs: [],
  activeTab: null,
  modalScope: 'workbench',
  flowDialogOpen: false,
  confirmReq: null,
  fileEdits: {},
  confirmFileEdits: async (ids) => {
    const selected = () => Object.entries(get().fileEdits).filter(([id]) => !ids || ids.includes(id)).map(([, edit]) => edit)
    if (!selected().some(edit => edit.dirty)) return true
    const dirtyPaths = selected().filter(edit => edit.dirty).map(edit => edit.path.split('/').pop() ?? edit.path)
    // D17：标题点名文件（canvas#leave「README.md 有未保存的修改」），多个文件时写个数。
    const title = dirtyPaths.length === 1 ? i18n.t('file.leaveTitleOne', { name: dirtyPaths[0] }) : i18n.t('file.leaveTitleMany', { count: dirtyPaths.length })
    return new Promise<boolean>(resolve => get().askConfirm({
      title, body: i18n.t('file.leaveBody'), variant: 'leave',
      confirmLabel: i18n.t('file.save'), confirmAction: 'saveFile', cancelAction: 'cancelFileEdits', skipRefresh: true,
      run: async () => {
        for (const edit of selected()) {
          if (edit.dirty && !await edit.save()) throw new Error(i18n.t('file.leaveSaveFailed'))
        }
        // Saving another file can take time; don't drop a newly edited buffer.
        if (selected().some(edit => edit.dirty)) throw new Error(i18n.t('file.leaveSaveFailed'))
      },
      alternate: {label: i18n.t('file.discard'), action: 'discardFileEdits', run: () => {
        for (const edit of selected()) if (edit.dirty) edit.discard?.()
      }},
      onClose: resolve,
    }))
  },
  setFileEdit: (id, edit) => set(s => {
    const next = { ...s.fileEdits }
    if (edit) next[id] = edit
    else delete next[id]
    return { fileEdits: next }
  }),
  toasts: [],
  setModalScope: (s) => set({ modalScope: s }),
  pushToast: (text, tone = 'info', opts) => {
    if (!text) return // A cancelled local navigation is not an error notification.
    const id = ++toastSeq
    set((s) => ({ toasts: [...s.toasts, { ...opts, id, text, tone }] }))
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  askConfirm: (r) => {
    const previous = get().confirmReq
    set({ confirmReq: r })
    previous?.onClose?.(false)
  },
  clearConfirm: (confirmed = false) => {
    const req = get().confirmReq
    set({ confirmReq: null })
    req?.onClose?.(confirmed)
  },
  // ui-audit 票 12（P2-13）：节点轨键盘入口——脉冲递增通知 NodeRail 展开聚焦。
  nodeRailPulse: 0,
  focusNodeRail: () => set((s) => ({ nodeRailPulse: s.nodeRailPulse + 1 })),
  intakeDraft: false,
  away: false,
  markAway: async () => {
    const s = useUiStore.getState()
    if (s.away) return
    try {
      await api.ownerAway()
      set({ away: true })
    } catch (e) {
      s.pushToast(errText(e), 'err')
    }
  },
  markBack: async () => {
    const s = useUiStore.getState()
    if (!s.away) return
    try {
      await api.ownerBack()
      set({ away: false })
      await s.refreshFast()
    } catch (e) {
      s.pushToast(errText(e), 'err')
    }
  },
  // 票 03（ADR 0088）：打开页签 = 打开画布；用量详情占着主区时先回到对话（画布要和对话并排或全宽）。
  openTab: (t) =>
    set((s) => ({
      tabs: s.tabs.some((x) => x.id === t.id) ? s.tabs : [...s.tabs, t],
      activeTab: t.id,
      canvasOpen: true,
      usageView: false,
    })),
  // 最后一个页签关闭时画布关闭（interaction §6）。未保存的文件先过离开确认（canvas#leave）。
  closeTab: (id) => get().closeTabs([id]),
  // owner-followups-1015 #05：closeTab 收成 closeTabs 的单元素情形——「关闭其他 / 关闭右侧」若逐个调 closeTab，
  // 每个脏文件各弹一次离开确认，第二个确认会盖掉第一个的 Promise。当前页签被关时落到剩下的最后一个
  //（关闭其他 / 关闭右侧时恰好就是右键的目标页签）。
  closeTabs: (ids) => {
    const current = get()
    const gone = new Set(ids)
    const paths = new Set(current.tabs.filter(t => gone.has(t.id) && t.kind === 'file').map(t => t.path))
    const edits = Object.entries(current.fileEdits).filter(([, edit]) => paths.has(edit.path))
    const close = () => {
      if (get().projectEpoch !== current.projectEpoch) return
      set(s => {
        const tabs = s.tabs.filter(t => !gone.has(t.id))
        const activeTab = s.activeTab != null && gone.has(s.activeTab) ? (tabs[tabs.length - 1]?.id ?? null) : s.activeTab
        return { tabs, activeTab, canvasOpen: tabs.length > 0 && s.canvasOpen }
      })
    }
    if (!edits.some(([, edit]) => edit.dirty)) close()
    else void get().confirmFileEdits(edits.map(([key]) => key)).then(allowed => { if (allowed) close() })
  },
  setActiveTab: (id) => set({ activeTab: id }),
  streams: {},
  plans: {},
  thinkings: {},
  streamDone: {},
  toolStreams: {},
  waitingSince: {},
  applyToolOutput: (d) =>
    set((s) => ({
      toolStreamKeys: d.seq == null ? s.toolStreamKeys : { ...s.toolStreamKeys,
        [`${d.agent_id}:${d.seq}`]: { agent_id: d.agent_id, seq: d.seq } },
      toolStreams: {
        ...s.toolStreams,
        [`${d.agent_id}:${d.seq ?? '·'}`]:
          ((s.toolStreams[`${d.agent_id}:${d.seq ?? '·'}`] ?? '') + d.text).slice(-TOOL_STREAM_CAP),
      },
    })),
  applyDelta: (d) =>
    set((s) => {
      if (d.done) {
        // 票 08：done 只标完结不清缓冲——气泡原地转为落位样式，
        // 持久化确认（refreshFast 拉到同 agent 消息）后才交接。
        const waitingSince = { ...s.waitingSince }
        delete waitingSince[d.agent_id] // 等网旗随回合收口兜底清除（票 NR-02）
        return {
          streamDone: {
            ...s.streamDone,
            [d.agent_id]: { afterEventId: s.timelineFacts?.latest_event_id ?? 0, at: Date.now(), hydrating: s.timelineFacts == null },
          },
          waitingSince,
        }
      }
      const plans = { ...s.plans }
      if (d.plan != null) plans[d.agent_id] = d.reset ? '' : (plans[d.agent_id] ?? '') + d.plan
      const streams = { ...s.streams }
      const thinkings = { ...s.thinkings }
      const waitingSince = { ...s.waitingSince }
      const cur = { ...(streams[d.agent_id] ?? {}) }
      const curT = { ...(thinkings[d.agent_id] ?? {}) }
      // reset=瞬时重试：本次调用的已收文本和思考作废（重试会重吐全文）
      if (d.reset) {
        cur[d.call] = ''
        curT[d.call] = ''
      } else {
        cur[d.call] = (cur[d.call] ?? '') + d.text
        if (d.thinking) curT[d.call] = (curT[d.call] ?? '') + d.thinking
      }
      streams[d.agent_id] = cur
      if (d.reset || Object.values(curT).some((x) => x.length > 0)) thinkings[d.agent_id] = curT
      // 票 NR-02：等网旗——true 帧记进入时刻（幂等：重进不重置起点），
      // 其余帧一律清除（正常增量/显式清旗帧都是「网回来了」的信号）。
      if (d.waiting) waitingSince[d.agent_id] = waitingSince[d.agent_id] ?? Date.now()
      else delete waitingSince[d.agent_id]
      return { streams, thinkings, plans, waitingSince }
    }),
  setSideLayout: (next) => {
    writeSideLayout(localStorage, next)
    set({ sideLayout: next })
  },
  toggleSidebar: () => {
    // 专注模式里侧栏是隐藏的：⌘B 先退出专注模式（恢复原布局），不在看不见的侧栏上翻状态（票 03）。
    if (get().focusSnapshot) { get().toggleFocusMode(); return }
    const cur = get().sideLayout
    get().setSideLayout({ ...cur, collapsed: !cur.collapsed })
  },
  openCanvas: (pane) => {
    get().openTab(paneTab(pane))
    const cur = get().canvasLayout
    const full = pane === 'files'
    if (cur.full !== full) get().setCanvasLayout({ ...cur, full })
  },
  // ⌥⌘B：开画布不新增页签（无页签时显示空态，interaction §6）；从用量详情打开时先回对话。
  setCanvasOpen: (v) => set(v ? { canvasOpen: true, usageView: false } : { canvasOpen: false }),
  toggleCanvas: () => {
    const s = get()
    s.setCanvasOpen(!(s.canvasOpen && !s.usageView))
  },
  setCanvasLayout: (next) => {
    writeCanvasLayout(localStorage, next)
    set({ canvasLayout: next })
  },
  toggleCanvasFull: () => {
    const s = get()
    if (!s.canvasOpen || s.usageView) {
      set({ canvasOpen: true, usageView: false })
      if (!s.canvasLayout.full) get().setCanvasLayout({ ...s.canvasLayout, full: true })
      return
    }
    get().setCanvasLayout({ ...s.canvasLayout, full: !s.canvasLayout.full })
  },
  showConversation: () => {
    const s = get()
    if (s.usageView) set({ usageView: false })
    if (s.canvasOpen && s.canvasLayout.full) get().setCanvasLayout({ ...s.canvasLayout, full: false })
  },
  openUsageDetail: () => set({ usageView: true }),
  // 只关画布、藏侧栏与节点轨，不动侧栏 / 画布的布局记忆：恢复时原样回来（R-06：纯界面态）。
  toggleFocusMode: () => {
    const s = get()
    if (s.focusSnapshot) set({ focusSnapshot: null, canvasOpen: s.focusSnapshot.canvasOpen })
    else set({ focusSnapshot: { canvasOpen: s.canvasOpen }, canvasOpen: false, usageView: false })
  },
  setFlowDialogOpen: (v) => set({ flowDialogOpen: v }),
  requestAgentScreen: () => set((s) => ({ agentScreenReq: s.agentScreenReq + 1 })),
  openPendingDialog: () => set({ pendingDialogOpen: true }),
  closePendingDialog: () => {
    const s = get()
    set({ pendingDialogOpen: false, dismissedPendingKeys: humanKeys(s.pending, s.reviewRows) })
  },
  noteReviewRows: (rows) => {
    const cur = get().reviewRows
    if (cur.length === rows.length && cur.every((r, i) => r.id === rows[i].id && r.status === rows[i].status)) return
    set({ reviewRows: rows })
  },
  syncPendingDialog: () => {
    const s = get()
    const keys = humanKeys(s.pending, s.reviewRows)
    if (keys.length === 0) {
      if (s.pendingDialogOpen || s.dismissedPendingKeys.length > 0) {
        set({ pendingDialogOpen: false, dismissedPendingKeys: [] })
      }
      return
    }
    // 已打开时不强制重开；收起后只有新 id 才再弹（点徽标走 openPendingDialog）。
    if (keys.some((k) => !s.dismissedPendingKeys.includes(k)) && !s.pendingDialogOpen) {
      set({ pendingDialogOpen: true })
    }
  },
  hidePending: (id) => set((s) => (s.hiddenPending.includes(id) ? s : { hiddenPending: [...s.hiddenPending, id] })),
  unhidePending: (id) => set((s) => (s.hiddenPending.includes(id) ? { hiddenPending: s.hiddenPending.filter((x) => x !== id) } : s)),
  notePendingError: (error) => set({ pendingError: error }),
  requestFileTree: (action) => {
    // 文件树快捷键先把树亮出来（票 11）；票 03 起树在画布「项目文件」页签里，不再写旧 `hexagon.rail`。
    // 已在文件页签（树就在左边）时不跳走。
    const s = get()
    const onTree = s.canvasOpen && (s.activeTab === 'files' || s.activeTab?.startsWith('file:'))
    if (!onTree) s.openTab(paneTab('files'))
    set((st) => ({ fileTreeReq: { n: (st.fileTreeReq?.n ?? 0) + 1, action } }))
  },
  requestSaveFile: () => set((s) => ({ saveFileReq: s.saveFileReq + 1 })),
  requestTimelineStick: () => set((s) => ({ timelineStickReq: s.timelineStickReq + 1 })),
  refresh: async () => {
    generation++
    const epoch = get().projectEpoch
    if (!get().projectRoot || get().projectGeneration === null) {
      try {
        const identity = await api.projectIdentity()
        if (get().projectEpoch !== epoch) return
        get().commitProjectIdentity(identity, epoch)
      } catch { return }
    }
    await get().invalidate()
  },
  refreshSlow: async (tags) => {
    // ui-audit 票 04（P1-6）：allSettled 按切片落地——单切片失败
    // 不再拖垮其余切片更新；失败切片一次 toast（不逐条刷）。
    // 附带修掉一个旧 bug：原先 projectInfo 内联 catch(()=>null) 会把
    // 「拉取失败」伪装成「空 info」，apply 时把 mode/packName 重置回默认值。
    const gen = generation
    const want = (t: SlowSlice) => !tags || tags.length === 0 || tags.includes(t)
    const settled = await Promise.allSettled([
      want('usage') ? api.usage() : Promise.resolve(undefined),
      want('usage') ? api.usageSeries('day', daysAgo(6)) : Promise.resolve(undefined),
      want('usage') ? api.usageContextPressure() : Promise.resolve(undefined),
      want('artifacts') ? api.artifacts() : Promise.resolve(undefined),
      want('team') ? api.team() : Promise.resolve(undefined),
      want('team') ? api.listProviders() : Promise.resolve(undefined),
      want('info') ? api.autonomy() : Promise.resolve(undefined),
      want('info') ? api.projectInfo() : Promise.resolve(undefined),
    ])
    // 异构 tuple：逐值 fulfilled 收窄（map+泛型在 strict 下推不出联合成员）
    const vals = settled.map((r) => (r.status === 'fulfilled' ? r.value : undefined))
    const [usage, series7d, pressure, artifacts, team, providersView, autonomy, info] = vals as [
      Awaited<ReturnType<typeof api.usage>> | undefined,
      Awaited<ReturnType<typeof api.usageSeries>> | undefined,
      Awaited<ReturnType<typeof api.usageContextPressure>> | undefined,
      Awaited<ReturnType<typeof api.artifacts>> | undefined,
      Awaited<ReturnType<typeof api.team>> | undefined,
      Awaited<ReturnType<typeof api.listProviders>> | undefined,
      Awaited<ReturnType<typeof api.autonomy>> | undefined,
      Awaited<ReturnType<typeof api.projectInfo>> | undefined,
    ]
    let avatars: Record<string, string> | undefined
    let avatarHashes: Record<string, string | null> | undefined
    if (team) {
      // 票 07：哈希未变不拉 data URL——稳态 team 失效只花 1 次 team()。
      const prev = useUiStore.getState().avatarHashes
      avatars = { ...useUiStore.getState().avatars }
      avatarHashes = {}
      await Promise.all(
        team.map(async (m) => {
          avatarHashes![m.id] = m.avatar_hash
          if (!m.avatar_hash) {
            delete avatars![m.id]
          } else if (m.avatar_hash !== prev[m.id]) {
            const u = await api.agentAvatar(m.id).catch(() => null)
            if (u) {
              avatars![m.id] = u
            } else {
              // 拉取失败：哈希记 null 强制下次重试，同时清掉陈旧图。
              delete avatars![m.id]
              avatarHashes![m.id] = null
            }
          }
        }),
      )
      // 离队 agent 的头像缓存一并清
      for (const id of Object.keys(avatars)) {
        if (!(id in avatarHashes)) delete avatars[id]
      }
    }
    if (gen !== generation) return // 代际守卫：旧项目的在飞响应落地前作废
    set((s) => ({
      ...(usage ? { usageTotal: usage.total, usageRows: usage.rows } : {}),
      ...(series7d ? { usageSeries7d: series7d } : {}),
      ...(pressure ? { contextPressure: pressure } : {}),
      ...(artifacts ? { artifacts } : {}),
      ...(team ? { team, avatars: avatars!, avatarHashes: avatarHashes! } : {}),
      ...(providersView ? { providers: providersView } : {}),
      ...(autonomy !== undefined ? { autonomy: autonomy || 'L0' } : {}),
      ...(info !== undefined && info !== null
        ? {
            mode: info.mode ?? ('pack' as const),
            fastRole: info.fastpath_role ?? null,
            packName: info.pack_name ?? null,
            projectName: info.name ?? s.projectName,
          }
        : {}),
    }))
    const bad = settled.find((r) => r.status === 'rejected')
    if (bad) useUiStore.getState().pushToast(errText((bad as PromiseRejectedResult).reason), 'err')
  },
  invalidate: async (...tags) => {
    const s = useUiStore.getState()
    await Promise.all([s.refreshFast(), s.refreshSlow(tags)])
    // D08: a refused acceptance may append no event. Explicit mutations still
    // invalidate the evidence view; background timeline polling does not.
    set((current) => ({ evidenceRevision: current.evidenceRevision + 1 }))
  },
  refreshFacts: async () => {
    const { projectRoot: root, projectEpoch: epoch } = get()
    if (!root) return
    const gen = generation
    const watched = Object.values(get().toolStreamKeys).slice(0, 500)
    try {
      const facts = await api.timelineFacts({ expected_project_root: root, watched_tool_streams: watched,
        watched_message_ids: watchedTimelineMessages(root, epoch) })
      if (generation !== gen || get().projectEpoch !== epoch || get().projectRoot !== root || facts.project_root !== root) return
      // Overlapping invalidation/poll responses must never roll a facts waterline back.
      if ((get().timelineFacts?.latest_event_id ?? -1) > facts.latest_event_id) return
      set(s => {
        const plans = { ...s.plans }
        for (const plan of facts.latest_plans) if (plans[plan.agent_id]?.trim() === plan.text.trim()) delete plans[plan.agent_id]
        const streamDone = { ...s.streamDone }, streams = { ...s.streams }, thinkings = { ...s.thinkings }
        const now = Date.now()
        for (const [agent, mark] of Object.entries(streamDone)) {
          if (mark.hydrating) {
            streamDone[agent] = { afterEventId: facts.latest_event_id, at: now }
            continue
          }
          const receipt = facts.latest_agent_messages.find(item => item.agent_id === agent)
          if ((receipt && receipt.event_id > mark.afterEventId) || now - mark.at > STREAM_HANDOFF_MS) {
            delete streamDone[agent]; delete streams[agent]; delete thinkings[agent]; delete plans[agent]
          }
        }
        const toolStreams = { ...s.toolStreams }, toolStreamKeys = { ...s.toolStreamKeys }
        for (const key of facts.settled_tool_streams) {
          const id = `${key.agent_id}:${key.seq}`
          delete toolStreams[id]; delete toolStreamKeys[id]
        }
        return { timelineFacts: facts, factsError: null, plans, streams, thinkings, streamDone, toolStreams, toolStreamKeys }
      })
    } catch (error) {
      if (generation === gen && get().projectEpoch === epoch) set({ factsError: errText(error) })
    }
  },
  refreshFast: async () => {
    const gen = generation
    const request = ++fastRequestSeq
    // Issue15: window/facts failure cannot suppress current pending cards. Each
    // projection owns its failure state; facts is independently race checked.
    const facts = get().refreshFacts()
    const draft = api.intakeDraftPending().catch(() => get().intakeDraft)
    const session = api.sessionSummary().then(value => ({ value })).catch(error => ({ error }))
    try {
      const [stages, pending, intakeDraft, sessionResult] = await Promise.all([api.stageStatus(), api.pendingQuestions(), draft, session])
      if (gen !== generation || request < lastAppliedFastRequest) return
      lastAppliedFastRequest = request
      fastFailStreak = 0
      set({ stages, pending, intakeDraft, statusError: false,
        sessionSummary: 'value' in sessionResult ? sessionResult.value : get().sessionSummary,
        sessionError: 'error' in sessionResult ? errText(sessionResult.error) : null,
      })
    } catch (error) {
      if (gen !== generation || request < lastAppliedFastRequest) return
      fastFailStreak++
      set({ statusError: true })
      if (fastFailStreak === FAST_FAIL_TOAST_AT) get().pushToast(errText(error), 'err')
    } finally { await facts }
  },
}))
