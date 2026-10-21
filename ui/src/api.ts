import type { ReconcileReport } from './gen/ReconcileReport'
import type { OpenWithApp } from './gen/OpenWithApp'
import type { ArtistProposal } from './gen/ArtistProposal'
import type { RejudgeOutcome } from './gen/RejudgeOutcome'
import type { ModelRequestRecovery } from './gen/ModelRequestRecovery'
import type { TimelineWindowMetadataRequest } from './gen/TimelineWindowMetadataRequest'
import type { TimelineWindowMetadata } from './gen/TimelineWindowMetadata'
import { mockTimelineWindow, mockTimelineFacts, mockTimelineNodes } from './timelineMock'
import { useUiStore } from './store'
import type { ProjectTurnDelta } from './gen/ProjectTurnDelta'
import type { ProjectToolOutputDelta } from './gen/ProjectToolOutputDelta'
import type { TimelineWindowRequest } from './gen/TimelineWindowRequest'
import type { TimelineWindowPage } from './gen/TimelineWindowPage'
import type { TimelineFactsRequest } from './gen/TimelineFactsRequest'
import type { TimelineFacts } from './gen/TimelineFacts'
import type { TimelineNodesRequest } from './gen/TimelineNodesRequest'
import type { TimelineNodesPage } from './gen/TimelineNodesPage'
import type { BrowserLabels } from './gen/BrowserLabels'
import type { BrowserSession } from './gen/BrowserSession'
import type { BrowserMode } from './gen/BrowserMode'
import type { BrowserPreview } from './gen/BrowserPreview'
import type { ElementRef } from './gen/ElementRef'
import type { NativePreviewTarget } from './gen/NativePreviewTarget'
import type { NativePreviewFrame } from './gen/NativePreviewFrame'
import type { DesignDirection } from './gen/DesignDirection'
import type { DesktopScreenshot } from './gen/DesktopScreenshot'
import type { ComputerModels } from './gen/ComputerModels'
import type { DesktopStatus } from './gen/DesktopStatus'
import type { DesktopControl } from './gen/DesktopControl'
import type { PermissionShapeSuggestion } from './gen/PermissionShapeSuggestion'
import type { ApprovalMode } from './gen/ApprovalMode'
import type { ApprovalModeStatus } from './gen/ApprovalModeStatus'
import type { DelegationStatus } from './gen/DelegationStatus'
import type { ManagedRecord } from './gen/ManagedRecord'
import type { ManagedSummary } from './gen/ManagedSummary'
import type { ExternalOperation } from './gen/ExternalOperation'
import type { DesktopPermissions } from './gen/DesktopPermissions'
import type { DesktopPermission } from './gen/DesktopPermission'
import type { ExperienceSourceRequest } from './gen/ExperienceSourceRequest'
import type { ExperienceSourceDocument } from './gen/ExperienceSourceDocument'
import type { ExperienceHistoryRequest } from './gen/ExperienceHistoryRequest'
import type { ExperienceHistoryPage } from './gen/ExperienceHistoryPage'
import type { ExperienceCuration } from './gen/ExperienceCuration'
import type { ExperienceRevocation } from './gen/ExperienceRevocation'
import type { ProjectSkillDocument } from './gen/ProjectSkillDocument'
import type { ExperienceLimits } from './gen/ExperienceLimits'
import type { ExperienceRecovery } from './gen/ExperienceRecovery'
import type { ExperienceEntryView } from './gen/ExperienceEntryView'
import type { ExperienceSubmission } from './gen/ExperienceSubmission'
import type { ExperienceRequest } from './gen/ExperienceRequest'
import type { ExperienceProposalView } from './gen/ExperienceProposalView'
import type { DataBoundary } from './gen/DataBoundary'
// 核 API 接缝：Tauri 环境走 invoke；浏览器开发环境用内置 mock 数据。
// UI 的唯一通道 = 这些命令 + 事件推送，没有旁路。类型对齐 api.rs 的 JSON 形状。

import { Channel, invoke } from '@tauri-apps/api/core'
import { CREATE_STEPS, type CreateStep } from './createProgress'
import i18n from './i18n'

// ---- IPC DTO（ADR 0054，票 06）：ui/src/gen/* 由 ts-rs 从 Rust DTO 生成 ----
// 手改禁地——字段要改改 Rust 侧，`cargo test` 重出声明，字段漂移由 tsc 抓。
// 词表字段（status/state/kind/mode）在 Rust 侧按 schema CHECK 钉了字面量联合。
import type { RepoFileSnapshot } from './gen/RepoFileSnapshot'
import type { ProjectIdentity } from './gen/ProjectIdentity'
import type { CmdError } from './gen/CmdError'
import type { AttachRef } from './gen/AttachRef'
import type { SandboxStatus } from './gen/SandboxStatus'
import type { QueuedCard } from './gen/QueuedCard'
import type { TurnDelta } from './gen/TurnDelta'
import type { ToolOutputDelta } from './gen/ToolOutputDelta'
import type { StageEvidence } from './gen/StageEvidence'
import type { ExceptionRequirement } from './gen/ExceptionRequirement'
import type { ExceptionRequest } from './gen/ExceptionRequest'
import type { ExceptionAcceptance } from './gen/ExceptionAcceptance'
import type { PerformanceBaselineConfirmation } from './gen/PerformanceBaselineConfirmation'
import type { QualityConfiguration } from './gen/QualityConfiguration'
import type { QualityCategory } from './gen/QualityCategory'
import type { StageRow } from './gen/StageRow'
import type { TimelineItem } from './gen/TimelineItem'
import type { TeamRow } from './gen/TeamRow'
import type { ArtifactRow } from './gen/ArtifactRow'
import type { UsageTotal } from './gen/UsageTotal'
import type { UsageRow } from './gen/UsageRow'
import type { UsageSummary } from './gen/UsageSummary'
import type { UsageBucket } from './gen/UsageBucket'
import type { ContextPressure } from './gen/ContextPressure'
import type { StageAction } from './gen/StageAction'
import type { OpenStageOutcome } from './gen/OpenStageOutcome'
import type { CheckResult } from './gen/CheckResult'
import type { CheckOutcome } from './gen/CheckOutcome'
import type { OverrideOutcome } from './gen/OverrideOutcome'
import type { InstallOutcome } from './gen/InstallOutcome'
import type { GrantOutcome } from './gen/GrantOutcome'
import type { PushOutcome } from './gen/PushOutcome'
import type { PushRequest } from './gen/PushRequest'
import type { PushTargets } from './gen/PushTargets'
import type { ReleaseTarget } from './gen/ReleaseTarget'
import type { ReleaseOutcome } from './gen/ReleaseOutcome'
import type { ReleaseTargetDraft } from './gen/ReleaseTargetDraft'
import type { FlagOutcome } from './gen/FlagOutcome'
import type { AdjudicateOutcome } from './gen/AdjudicateOutcome'
import type { ReturnSummary } from './gen/ReturnSummary'
import type { PermissionRuleRow } from './gen/PermissionRuleRow'
import type { SkillRow } from './gen/SkillRow'
import type { McpServiceRow } from './gen/McpServiceRow'
import type { ProposalRow } from './gen/ProposalRow'
import type { ProjectInfo } from './gen/ProjectInfo'
import type { RecentProject } from './gen/RecentProject'
import type { TurnOutcome } from './gen/TurnOutcome'
import type { DirReport } from './gen/DirReport'
import type { RoleDef } from './gen/RoleDef'
import type { RoleTemplate } from './gen/RoleTemplate'
import type { ExtSkillRow } from './gen/ExtSkillRow'
import type { ImportReport } from './gen/ImportReport'
import type { McpEntryRow } from './gen/McpEntryRow'
import type { McpSpec } from './gen/McpSpec'
import type { ExtMcpRow } from './gen/ExtMcpRow'
import type { PackDef } from './gen/PackDef'
import type { StageDef } from './gen/StageDef'
import type { AgentPatch } from './gen/AgentPatch'
import type { AgentDetail } from './gen/AgentDetail'
import type { ModelEntry } from './gen/ModelEntry'
import type { ProviderDef } from './gen/ProviderDef'
import type { KeyProbe } from './gen/KeyProbe'
import type { ProviderPreset } from './gen/ProviderPreset'
import type { ProviderView } from './gen/ProviderView'
import type { ProvidersView } from './gen/ProvidersView'
import type { SlotBinding } from './gen/SlotBinding'
import type { DiagRecord } from './gen/DiagRecord'
import type { PromptEntry } from './gen/PromptEntry'
import type { TranslateOutcome } from './gen/TranslateOutcome'
import type { CreateProjectOpts } from './gen/CreateProjectOpts'
import type { CreateStep as CreateStepDto } from './gen/CreateStep'
import type { Event } from './gen/Event'
import type { EventKind } from './gen/EventKind'
import type { MessageRow } from './gen/MessageRow'
import type { MessageToken } from './gen/MessageToken'
import type { ExportFilter } from './gen/ExportFilter'
import type { RepoEntry } from './gen/RepoEntry'
import type { BriefQA } from './gen/BriefQA'
import type { BriefQuestion } from './gen/BriefQuestion'
import type { RoleSeedDraft } from './gen/RoleSeedDraft'
import type { SessionSummary } from './gen/SessionSummary'
import type { CapabilityStatus } from './gen/CapabilityStatus'
import type { OneTimeTrigger } from './gen/OneTimeTrigger'
import type { WorktreeAttachment } from './gen/WorktreeAttachment'
import type { WorktreeOperation } from './gen/WorktreeOperation'
import type { WorktreeRequest } from './gen/WorktreeRequest'
import type { SessionRecord } from './gen/SessionRecord'

/** 旧名薄壳——新代码直接用 QueuedCard。 */
export type PendingQuestion = QueuedCard
/** 旧名薄壳——新代码直接用 StageDef。 */
export type PackStage = StageDef

export type {
  StageEvidence, CmdError, TurnDelta, ToolOutputDelta, StageRow, TimelineItem, QueuedCard, TeamRow, ArtifactRow,
  UsageTotal, UsageRow, UsageSummary, UsageBucket, ContextPressure, StageAction, OpenStageOutcome,
  CheckResult, CheckOutcome, OverrideOutcome, InstallOutcome, PushOutcome, PushRequest, PushTargets,
  FlagOutcome, AdjudicateOutcome, ReturnSummary, ProposalRow, ProjectInfo,
  RecentProject, TurnOutcome, DirReport, RoleDef, PackDef, StageDef, AgentPatch,
  PermissionRuleRow, SkillRow, McpServiceRow,
  AgentDetail, ModelEntry, ProviderDef, ProviderView, ProvidersView, SlotBinding, ProviderPreset,
  CreateProjectOpts, CreateStepDto, Event, EventKind, MessageRow, MessageToken, ExportFilter,
  RoleTemplate, ExtSkillRow, ImportReport, McpEntryRow, McpSpec, ExtMcpRow,
  RepoEntry, DiagRecord, PromptEntry, TranslateOutcome,
  BriefQA, BriefQuestion, RoleSeedDraft,
  SessionSummary, CapabilityStatus,
  OneTimeTrigger,
  WorktreeAttachment, WorktreeOperation, WorktreeRequest, SessionRecord,
  ReleaseTarget, ReleaseTargetDraft, ReleaseOutcome,
}


export const isTauri = '__TAURI_INTERNALS__' in window || '__TAURI__' in window

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // I1 / 2026-10-06: q1 is local to each project DB. Capture the host-issued
  // incarnation before dispatch; a delayed approval must not target a new q1.
  if (isTauri) return invoke<T>(cmd, { ...args, _project: args && '_project' in args ? args._project : currentProjectIdentity() })
  // mock 只可达于 dev 构建：import.meta.env.DEV 在产物里是字面量 false，
  // 此分支为死码被 tree-shake，~560 行夹具不进生产包（arch-review 附录 B4
  // 核验：原先走运行时 isTauri 判，摇不掉）。组件侧的 isTauri 是 UI 显隐
  // 判定，与本门控职责不同，保留运行时检查。
  if (import.meta.env.DEV) return mock<T>(cmd, args)
  throw new Error('non-Tauri production build has no backend')
}

function currentProjectIdentity(): ProjectIdentity | null {
  const { projectRoot, projectGeneration } = useUiStore.getState()
  return projectRoot !== null && projectGeneration !== null
    ? { project_root: projectRoot, generation: projectGeneration } : null
}

// ---- 错误信封（ADR 0054，arch-review 票 06）----
// 命令 Err 面出列即 {code,message}：code 是 core 侧变体稳定标识，
// message 透传含参数的 Display 原文。渲染规则：已知 code 走
// `errors.<code>` i18n key，未知 code 渲染 message——i18n 表只覆盖
// 高频可修码，长尾直通（被否替代：全码表七语翻译——维护面爆炸且
// 长尾文案价值低）。

/// 任意 invoke 拒绝值 → 信封。旧串/非信封对象兜底 code:"unknown"。
export function asCmdError(e: unknown): CmdError {
  if (typeof e === 'object' && e !== null) {
    const o = e as { code?: unknown; message?: unknown }
    if (typeof o.message === 'string') {
      return {
        code: typeof o.code === 'string' ? o.code : 'unknown',
        message: o.message,
      }
    }
  }
  return { code: 'unknown', message: String(e) }
}

/// 面向用户的错误文案唯一出口：组件一律用它，不再 `String(e)`。
export function errText(e: unknown): string {
  if (e instanceof ProjectChangeCancelled) return ''
  const { code, message } = asCmdError(e)
  const key = `errors.${code}`
  return i18n.exists(key) ? i18n.t(key) : message
}

class ProjectChangeCancelled extends Error {}

// ---- 回合流式 delta（turn-streaming 票 03）----
// 瞬时增量通道：payload 不落库；done=true 是流终信号（成败都发），
// reset=true 表示瞬时重试、本轮已收文本作废重起。

// exec-cards 票 04：bash 输出瞬时通道——与 turn-delta 同纪律
//（不落库只展示，tool_result 仍是持久层）。
export async function onToolOutput(cb: (d: ToolOutputDelta) => void): Promise<() => void> {
  if (!isTauri) return async () => {}
  const identity = currentProjectIdentity()
  if (!identity) return () => {}
  const { listen } = await import('@tauri-apps/api/event')
  return listen<ProjectToolOutputDelta>('tool-output', (e) => {
    if (sameProjectIdentity(e.payload.identity, identity) && sameProjectIdentity(identity, currentProjectIdentity())) cb(e.payload.delta)
  })
}

/// 订阅回合 delta；浏览器 dev 无推送通道，返回 no-op 退订。
export async function onTurnDelta(cb: (d: TurnDelta) => void): Promise<() => void> {
  if (!isTauri) return () => {}
  const identity = currentProjectIdentity()
  if (!identity) return () => {}
  const { listen } = await import('@tauri-apps/api/event')
  // Benchmark I1: admission alone cannot revoke queued native events. Bind both
  // delivery and the 50ms coalescing flush to the originating host generation.
  const batch = batchTurnDeltas(d => { if (sameProjectIdentity(identity, currentProjectIdentity())) cb(d) })
  const unlisten = await listen<ProjectTurnDelta>('turn-delta', (e) => {
    if (sameProjectIdentity(e.payload.identity, identity) && sameProjectIdentity(identity, currentProjectIdentity())) batch.push(e.payload.delta)
  })
  return () => { unlisten(); batch.cancel() }
}

function sameProjectIdentity(left: ProjectIdentity | null | undefined, right: ProjectIdentity | null | undefined): boolean {
  return !!left && !!right && left.project_root === right.project_root && left.generation === right.generation
}

// 2026-10-01 native profiling: per-token IPC caused thousands of synchronous
// Markdown renders. Coalesce adjacent data frames; protocol boundaries stay ordered.
export function batchTurnDeltas(cb: (d: TurnDelta) => void) {
  let pending: TurnDelta | undefined
  let timer: ReturnType<typeof setTimeout> | undefined
  const flush = () => {
    clearTimeout(timer)
    timer = undefined
    const value = pending
    pending = undefined
    if (value) cb(value)
  }
  return {
    push(d: TurnDelta) {
      if (d.reset || d.done || d.waiting) { flush(); cb(d); return }
      if (pending && (pending.agent_id !== d.agent_id || pending.stage_run_id !== d.stage_run_id || pending.call !== d.call)) flush()
      pending = pending ? { ...d, text: pending.text + d.text,
        thinking: pending.thinking + d.thinking,
        plan: pending.plan == null && d.plan == null ? undefined : (pending.plan ?? '') + (d.plan ?? ''),
      } : { ...d }
      timer ??= setTimeout(flush, 50)
    },
    cancel() { clearTimeout(timer); timer = undefined; pending = undefined },
  }
}

// Project IPCs have one synchronous invalidation boundary, including callers in
// the wizard/start screen. Reopening the same canonical root is still a new epoch.
async function switchProject<T>(operation: (source: ProjectIdentity | null) => Promise<T>, closing = false): Promise<T> {
  const store = useUiStore.getState()
  // I1: the no-edit path must still invalidate identity synchronously.
  if (Object.values(store.fileEdits).some(edit => edit.dirty) && !await store.confirmFileEdits()) {
    throw new ProjectChangeCancelled(i18n.t('file.leaveCancelled'))
  }
  if (useUiStore.getState().projectEpoch !== store.projectEpoch || useUiStore.getState().projectRoot !== store.projectRoot) {
    throw new ProjectChangeCancelled(i18n.t('file.leaveCancelled'))
  }
  const source = currentProjectIdentity()
  store.beginProjectSwitch()
  const epoch = useUiStore.getState().projectEpoch
  let succeeded = false
  try {
    const result = await operation(source)
    succeeded = true
    return result
  } finally {
    if (epoch === useUiStore.getState().projectEpoch && !(closing && succeeded)) {
      try {
        const identity = await call<ProjectIdentity | null>('project_identity')
        useUiStore.getState().commitProjectIdentity(identity, epoch)
      } catch { /* no active project; null keeps old content hidden */ }
    }
  }
}

export const api = {
  projectIdentity: () => call<ProjectIdentity | null>('project_identity'),
  sessionSummary: () => call<SessionSummary>('session_summary'),
  capabilityStatuses: () => call<CapabilityStatus[]>('capability_statuses'),
  sessionPause: () => call<void>('session_pause'),
  sessionReconnect: () => call<void>('session_reconnect'),
  sessionResume: (runId: string | null) => call<void>('session_resume', { runId }),
  sessionTriggers: () => call<OneTimeTrigger[]>('session_triggers'),
  // Core derives model slot, permission package and trace from the current
  // turn; IPC only carries the idempotence key and the fire time.
  sessionScheduleOnce: (triggerId: string, fireAt: number) =>
    call<OneTimeTrigger>('session_schedule_once', { triggerId, fireAt }),
  // Gap D1: retract a booked continue-later; core refuses once it has fired
  // (`trigger_already_fired`).
  sessionCancelTrigger: (triggerId: string) => call<OneTimeTrigger>('session_cancel_trigger', { triggerId }),
  sessionContinueTrigger: (triggerId: string) => call<OneTimeTrigger>('session_continue_trigger', { triggerId }),
  // Wayfinder-07 Addendum A: requests only enqueue a `worktree` card; the
  // pending dock's confirm/reject answer it and core owns the git write.
  sessionRequestWorktree: (operation: WorktreeOperation, path?: string) =>
    call<WorktreeRequest>('session_request_worktree', { operation, path: path ?? null }),
  confirmWorktree: (qid: string) => call<SessionRecord>('confirm_worktree', { qid }),
  rejectWorktree: (qid: string) => call<void>('reject_worktree', { qid }),
  sessionVerifyWorktree: () => call<SessionRecord>('session_verify_worktree'),
  timelineWindowMetadata: (request: TimelineWindowMetadataRequest) => call<TimelineWindowMetadata>('timeline_window_metadata', { request }),
  timelineWindow: (request: TimelineWindowRequest) => call<TimelineWindowPage>('timeline_window', { request }),
  timelineFacts: (request: TimelineFactsRequest) => call<TimelineFacts>('timeline_facts', { request }),
  timelineNodes: (request: TimelineNodesRequest) => call<TimelineNodesPage>('timeline_nodes', { request }),
  exportTimelineItems: async (kinds?: string[]) => {
    const epoch = useUiStore.getState().projectEpoch
    const root = useUiStore.getState().projectRoot
    const items: TimelineItem[] = []
    let after: number | undefined
    while (true) {
      const page = await call<TimelineItem[]>('timeline', { after: after ?? null, limit: 500 })
      // Issue15: an export may span several IPC reads; never stitch workspaces.
      if (useUiStore.getState().projectEpoch !== epoch || useUiStore.getState().projectRoot !== root) throw new Error('Project changed during timeline export')
      if (!page.length) break
      items.push(...page.filter(item => !kinds || kinds.includes(item.event.kind)))
      const cursor = page.at(-1)!.event.id
      if (after != null && cursor <= after) throw new Error('Timeline export cursor did not advance')
      after = cursor
      if (page.length < 500) break
    }
    return items
  },
  dataBoundary: () => call<DataBoundary>('data_boundary'),
  ping: () => call<string>('core_ping'),
  openProject: (dir: string, name: string, roles: [string, string][], packJson?: string) =>
    switchProject(source => call<void>('open_project', { dir, name, roles, packJson: packJson ?? null, _project: source })),
  timeline: (after?: number, limit = 500) =>
    call<TimelineItem[]>('timeline', { after: after ?? null, limit }),
  browserSelectionStart: (expectedProjectRoot: string, sessionId: string, labels: { element: string; region: string; done: string; hint: string }) =>
    call<void>('browser_selection_start', { expectedProjectRoot, sessionId, labels }),
  browserSelectionPoll: (expectedProjectRoot: string, sessionId: string) =>
    call<ElementRef[]>('browser_selection_poll', { expectedProjectRoot, sessionId }),
  browserSelectionDiscard: (expectedProjectRoot: string, ids: string[]) =>
    call<void>('browser_selection_discard', { expectedProjectRoot, ids }),
  sendElementMessage: (body: string, attachments: AttachRef[], elementIds: string[], expectedProjectRoot: string) =>
    call<number>('send_message', { body, attachments, elementIds, expectedProjectRoot }),
  sendMessage: (body: string, attachments: AttachRef[] = []) =>
    call<number>('send_message', { body, attachments }),
  // 票 03：粘贴/拖拽图片先暂存 .hexagon/inbox/，发送时带引用
  stageAttachment: (name: string, bytes: Uint8Array) =>
    call<AttachRef>('stage_attachment', { name, bytes: Array.from(bytes) }),
  discardAttachments: (refs: AttachRef[]) =>
    call<void>('discard_attachments', { refs }),
  // ui-audit-2 票 09：composer # 路径补全（仓根有界遍历，≤60 条）
  repoPaths: (query: string) => call<string[]>('repo_paths', { query }),
  // 票 11：右栏文件树。读/建/写都落仓根，路径由核围栏。
  listRepoDir: (rel = '') => call<RepoEntry[]>('list_repo_dir', { rel }),
  readRepoFile: (path: string) => call<string>('read_repo_file', { path }),
  readRepoFileSnapshot: (path: string, expectedProjectRoot: string) =>
    call<RepoFileSnapshot>('read_repo_file_snapshot', { path, expectedProjectRoot }),
  /** 文件在 git HEAD 的版本（文件页对比基线）。非仓/未跟踪 → null。 */
  repoFileHead: (path: string) => call<string | null>('repo_file_head', { path }),
  writeRepoFile: (snapshot: RepoFileSnapshot, content: string) => call<void>('write_repo_file', { snapshot, content }),
  createRepoFile: (path: string) => call<void>('create_repo_file', { path }),
  createRepoDir: (path: string) => call<void>('create_repo_dir', { path }),
  /** owner-followups-1015 #07：在访达 / 资源管理器 / 文件管理器中显示（控制组；根约束在 core，越界 errors.reveal_outside_project）。 */
  revealRepoPath: (path: string) => call<void>('reveal_repo_path', { path }),
  /** owner-followups-1015 #31「打开方式」：本机可打开该路径的 IDE / 编辑器（白名单 ∩ 系统登记；空串 = 项目根）。 */
  openWithCandidates: (path: string) => call<OpenWithApp[]>('open_with_candidates', { path }),
  /** 用指定应用打开（控制组）；core 重判根约束 + 白名单，越界 errors.open_with_outside_project。 */
  openWith: (path: string, appId: string) => call<void>('open_with', { path, appId }),
  permissionShapeSuggestion: (questionId: string) => call<PermissionShapeSuggestion | null>('permission_shape_suggestion', { questionId }),
  approvalMode: () => call<ApprovalModeStatus>('approval_mode'),
  setApprovalMode: (mode: ApprovalMode, expectedProjectRoot: string) => call<ApprovalModeStatus>('set_approval_mode', { mode, expectedProjectRoot }),
  projectDelegation: () => call<DelegationStatus>('project_delegation'),
  setProjectDelegation: (enabled: boolean, expectedProjectRoot: string) => call<DelegationStatus>('set_project_delegation', { enabled, expectedProjectRoot }),
  managedHistory: () => call<ManagedRecord[]>('managed_history'),
  managedExternalOps: () => call<ExternalOperation[]>('managed_external_ops'),
  /** gap A2/A3：托管时段、项目预算与本次托管花费（read 组）。 */
  managedSummary: () => call<ManagedSummary>('managed_summary'),
  /** gap A4：把 UI 生成的托管报告 Markdown 写到负责人在保存对话框里选的路径。 */
  exportManagedReport: (path: string, markdown: string) => call<null>('export_managed_report', { path, markdown }),
  managedProcessing: () => call<PendingQuestion[]>('managed_processing'),
  setProjectDelegationPaused: (paused: boolean, expectedProjectRoot: string) => call<DelegationStatus>('set_project_delegation_paused', { paused, expectedProjectRoot }),
  stopModelRequest: (questionId: string) => call<null>('stop_model_request', { questionId }),
  designDirection: () => call<DesignDirection>('design_direction'),
  chooseDesignDirection: (questionId: string, revision: number, optionId?: string | null, existingGuidance?: string | null) => call<DesignDirection>('choose_design_direction', { questionId, revision, optionId: optionId ?? null, existingGuidance: existingGuidance ?? null }),
  desktopPreviewTarget: (expectedProjectRoot: string) => call<NativePreviewTarget | null>('desktop_preview_target', { expectedProjectRoot }),
  desktopPreview: (expectedProjectRoot: string, windowId: number, processId: number) => call<NativePreviewFrame>('desktop_preview', { expectedProjectRoot, windowId, processId }),
  desktopPreviewStop: (expectedProjectRoot: string) => call<void>('desktop_preview_stop', { expectedProjectRoot }),
  desktopPreviewFocus: (expectedProjectRoot: string, windowId: number, processId: number) => call<void>('desktop_preview_focus', { expectedProjectRoot, windowId, processId }),
  desktopScreenshot: (name: string, expectedProjectRoot: string) => call<DesktopScreenshot>('desktop_screenshot', { name, expectedProjectRoot }),
  browserStatus: (expectedProjectRoot: string) => call<BrowserSession | null>('browser_status', { expectedProjectRoot }),
  browserOpen: (expectedProjectRoot: string, mode: BrowserMode, labels: BrowserLabels) => call<BrowserSession>('browser_open', { expectedProjectRoot, mode, labels }),
  browserDetach: (expectedProjectRoot: string, sessionId: string) => call<void>('browser_detach', { expectedProjectRoot, sessionId }),
  browserPreview: (expectedProjectRoot: string, sessionId: string) => call<BrowserPreview>('browser_preview', { expectedProjectRoot, sessionId }),
  browserFocus: (expectedProjectRoot: string, sessionId: string) => call<void>('browser_focus', { expectedProjectRoot, sessionId }),
  desktopStatus: () => call<DesktopStatus>('desktop_status'),
  computerModels: () => call<ComputerModels>('computer_models'),
  desktopControl: (action: DesktopControl, expectedProjectRoot: string) => call<DesktopStatus>('desktop_control', { action, expectedProjectRoot }),
  /** 功能缺口 C1：切换项目前暂停本项目电脑操作并留 checkpoint（回来后「继续」= desktop_control resume）。 */
  desktopLeaveProject: (expectedProjectRoot: string) => call<DesktopStatus>('desktop_leave_project', { expectedProjectRoot }),
  desktopPermissions: () => call<DesktopPermissions>('desktop_permissions'),
  desktopOpenSettings: (permission: DesktopPermission) => call<void>('desktop_open_settings', { permission }),
  sandboxStatus: () => call<SandboxStatus>('sandbox_status'),
  allowProjectPermission: (questionId: string) => call<void>('allow_project_permission', { questionId }),
  answerPermission: (questionId: string, allow: boolean, rememberShape?: string, scope = 'activation') =>
    call<void>('answer_permission', { questionId, allow, rememberShape: rememberShape ?? null, scope }),
  advance: () => call<StageAction>('advance'),
  openStage: (seq: number) => call<OpenStageOutcome>('open_stage', { seq }),
  runChecks: (expectedProjectRoot?: string) => call<CheckOutcome>('run_checks', { expectedProjectRoot: expectedProjectRoot ?? null }),
  stamp: () => call<StageAction>('stamp'),
  rewind: (toSeq: number) => call<StageAction>('rewind', { toSeq }),
  skip: () => call<StageAction>('skip'),
  skipReview: (artifactKind: string) => call<void>('skip_review', { artifactKind }),
  pause: () => call<void>('pause'),
  resume: () => call<void>('resume'),
  sleepAll: () => call<void>('sleep_all'),
  artifacts: () => call<ArtifactRow[]>('artifacts'),
  artifactContent: (path: string) => call<string>('artifact_content', { path }),
  artifactContentAt: (path: string, version: number) =>
    call<string | null>('artifact_content_at', { path, version }),
  setAgentSleeping: (agentId: string, sleeping: boolean) =>
    call<void>('set_agent_sleeping', { agentId, sleeping }),
  /** gap D2「移出团队」：core 软删 + 守门（agent_busy / agent_has_pending / agent_is_fastpath / agent_not_found）。 */
  removeAgent: (agentId: string) => call<void>('remove_agent', { agentId }),
  team: () => call<TeamRow[]>('team'),
  requestAcceptanceException: (expected: string) => call<ExceptionRequest>('request_acceptance_exception', { expected }),
  confirmPerformanceBaseline: (measurement: number, expected: string, reason: string) => call<PerformanceBaselineConfirmation>('confirm_performance_baseline', { measurement, expected, reason }),
  qualityConfiguration: () => call<QualityConfiguration>('quality_configuration'),
  cancelQualityRevalidation: (question: string, expectedProjectRoot: string) => call<void>('cancel_quality_revalidation', { question, expectedProjectRoot }),
  confirmQualityRevalidation: (question: string, expected: string, expectedProjectRoot: string) => call<StageAction>('confirm_quality_revalidation', { question, expected, expectedProjectRoot }),
  updateQualityCommands: (seq: number, expected: string, commands: Partial<Record<QualityCategory, string>>, expectedProjectRoot: string) => call<QualityConfiguration>('update_quality_commands', { seq, expected, commands, expectedProjectRoot }),
  acceptDeliveryException: (question: string, expected: string, selected: ExceptionRequirement[], reason: string) => call<ExceptionAcceptance>('accept_delivery_exception', { question, expected, selected, reason }),
  cancelAcceptanceException: (question: string) => call<void>('cancel_acceptance_exception', { question }),
  stageEvidence: () => call<StageEvidence | null>('stage_evidence'),
  stageStatus: () => call<StageRow[]>('stage_status'),
  pendingQuestions: () => call<PendingQuestion[]>('pending_questions'),
  modelRequestRecovery: (qid: string) => call<ModelRequestRecovery>('model_request_recovery', { qid }),
  retryModelRequest: (qid: string, expectedProjectRoot: string) => call<ModelRequestRecovery>('retry_model_request', { qid, expectedProjectRoot }),
  retryModelRequestWithChatModel: (qid: string, expectedProjectRoot: string) => call<ModelRequestRecovery>('retry_model_request_with_chat_model', { qid, expectedProjectRoot }),
  rejudgeAfterModelFailure: (qid: string, expectedProjectRoot: string) => call<RejudgeOutcome>('rejudge_after_model_failure', { qid, expectedProjectRoot }),
  stopWizardGeneration: () => call<void>('stop_wizard_generation'),
  resolveModelRequestChoice: (qid: string, followNew: boolean, expectedProjectRoot: string) => call<ModelRequestRecovery>('resolve_model_request_choice', { qid, followNew, expectedProjectRoot }),
  // ui-audit-2 票 03：已记权限规则审计面
  permissionRules: () => call<PermissionRuleRow[]>('permission_rules'),
  revokePermissionRule: (ruleId: string) =>
    call<void>('revoke_permission_rule', { ruleId }),
  // ui-audit-2 票 04：技能清单 + 全局静音（"*" 会话键）
  listSkills: () => call<SkillRow[]>('list_skills'),
  // global-config 票 03：技能包详情 + 全局技能编辑（无项目也可用）
  skillFiles: (name: string) => call<string[]>('skill_files', { name }),
  readSkillFile: (name: string, rel: string) =>
    call<string>('read_skill_file', { name, rel }),
  saveGlobalSkill: (name: string, description: string, body: string) =>
    call<void>('save_global_skill', { name, description, body }),
  // 票 04：外部技能扫描导入 + 包安装（文件夹/ZIP）
  scanExternalSkills: () => call<ExtSkillRow[]>('scan_external_skills'),
  importSkills: (paths: string[]) => call<ImportReport>('import_skills', { paths }),
  installSkillPath: (path: string) => call<string>('install_skill_path', { path }),
  setSkillMuted: (name: string, enabled: boolean) =>
    call<void>('set_skill_muted', { name, enabled }),
  setUiLanguage: (code: string) => call<void>('set_ui_language', { code }),
  promptCatalog: () => call<PromptEntry[]>('prompt_catalog'),
  translatePrompts: (lang: string, force: boolean) =>
    call<TranslateOutcome>('translate_prompts', { lang, force }),
  // ui-audit-2 票 06：MCP 服务实况（.hexagon/mcp.json 配置 → 宿主状态）
  mcpServices: () => call<McpServiceRow[]>('mcp_services'),
  // 票 05：MCP 全局清单（无项目可用；配置清单≠授权）
  mcpEntries: () => call<McpEntryRow[]>('list_mcp_entries'),
  saveMcpService: (spec: McpSpec) => call<void>('save_mcp_service', { spec }),
  deleteMcpService: (name: string) => call<void>('delete_mcp_service', { name }),
  // 票 06：本机 MCP 扫描导入 + 市场
  scanExternalMcp: () => call<ExtMcpRow[]>('scan_external_mcp'),
  importMcp: (references: string[]) => call<ImportReport>('import_mcp', { references }),
  openMcpMarket: () => call<void>('open_mcp_market'),
  openBrowserExtensionStore: () => call<void>('open_browser_extension_store'),
  usage: () => call<UsageSummary>('usage'),
  usageContextPressure: () => call<ContextPressure>('usage_context_pressure'),
  // 票 12（2026-10-14 负责人裁决）：所选范围内的 agent×model×stage 明细，供模型环 / 按阶段用
  usageRows: (from?: string | null, to?: string | null) =>
    call<UsageRow[]>('usage_rows', { from: from ?? null, to: to ?? null }),
  usageSeries: (granularity: 'day' | 'hour' = 'day', from?: string | null, to?: string | null) =>
    call<UsageBucket[]>('usage_series', { granularity, from: from ?? null, to: to ?? null }),
  setUsageLimit: (limitCents: number | null) =>
    call<void>('set_usage_limit', { limitCents }),
  setLogEnabled: (enabled: boolean) => call<void>('set_log_enabled', { enabled }),
  logEnabled: () => call<boolean>('log_enabled'),
  // diagnostic-records 票 01：设置「日志」页读回。project=null → 只回
  // 宿主记录；cls=null → 全部四类。Debug 过滤在 core 读回侧收口。
  diagnosticRecords: (project: string | null, cls: string | null) =>
    call<DiagRecord[]>('diagnostic_records', { project, class: cls }),
  autonomy: () => call<string>('autonomy'),
  setAutonomy: (level: string) => call<void>('set_autonomy', { level }),
  // ui-audit-2 票 07：审查者档位（live 仅在 ≥L1 生效）
  reviewerMode: () => call<string>('reviewer_mode'),
  setReviewerMode: (mode: string) => call<void>('set_reviewer_mode', { mode }),
  ownerAway: () => call<void>('owner_away'),
  ownerBack: () => call<ReturnSummary>('owner_back'),
  // ---- 决策卡动作 ----
  rejectStamp: (stage?: string | null, note?: string | null) =>
    call<StageAction>('reject_stamp', { stage: stage ?? null, note: note ?? null }),
  adjudicateFlag: (qid: string, agree: boolean) =>
    call<AdjudicateOutcome>('adjudicate_flag', { qid, agree }),
  proposals: () => call<ProposalRow[]>('proposals'),
  curateLegacyExperience: (request: ExperienceCuration) => call<ExperienceSubmission>('curate_legacy_experience', { request }),
  revokeExperience: (request: ExperienceRevocation) => call<ExperienceEntryView>('revoke_experience', { request }),
  projectSkillDocument: (skill: string) => call<ProjectSkillDocument>('project_skill_document', { skill }),
  saveProjectSkillDocument: (document: ProjectSkillDocument) => call<ProjectSkillDocument>('save_project_skill_document', { document }),
  experienceLimits: () => call<ExperienceLimits>('experience_limits'),
  setExperienceLimits: (limits: ExperienceLimits) => call<ExperienceLimits>('set_experience_limits', { limits }),
  recoverExperience: () => call<ExperienceRecovery[]>('recover_experience'),
  experienceSourceDocument: (request: ExperienceSourceRequest) => call<ExperienceSourceDocument>('experience_source_document', { request }),
  experienceHistory: (request: ExperienceHistoryRequest) => call<ExperienceHistoryPage>('experience_history', { request }),
  experienceEntries: (skill: string) => call<ExperienceEntryView[]>('experience_entries', { skill }),
  experienceProposal: (proposalId: string) => call<ExperienceProposalView | null>('experience_proposal', { proposalId }),
  proposeExperienceEntry: (agentId: string, request: ExperienceRequest) => call<ExperienceSubmission>('propose_experience_entry', { agentId, request }),
  // ui-audit-2 票 08：owner 对 in_review 提案的裁决（署名 owner——复审
  //  agent 从无工具可达 review()，唯一裁决面就是负责人）。
  reviewProposal: (proposalId: string, pass: boolean, reason: string) =>
    call<void>('review_proposal', { proposalId, pass, reason }),
  confirmProposal: (qid: string) => call<string>('confirm_proposal', { qid }),
  invariantCheck: () => call<number>('invariant_check'),
  policydevPropose: (
    edits: Record<string, unknown>[],
    scenario: Record<string, unknown>,
    motive: string,
  ) => call<string>('policydev_propose', { edits, scenario, motive }),
  rejectProposal: (qid: string, reason: string) =>
    call<void>('reject_proposal', { qid, reason }),
  rollbackProposal: (proposalId: string) =>
    call<void>('rollback_proposal', { proposalId }),
  // owner-followups-1015 #13：已「允许本项目」同一（远程, 基线分支）时宿主当场推送，pushed 为回执。
  requestPush: (remote: string) => call<PushRequest>('request_push', { remote }),
  /** remember = 推送卡上的「允许本项目」（记 远程 + 分支）。 */
  confirmPush: (qid: string, remember = false) => call<PushOutcome>('confirm_push', { qid, remember }),
  /** ⌘K「推送」目标：基线上游远程；无上游时由负责人在 remotes 里选。 */
  pushTargets: () => call<PushTargets>('push_targets'),
  rejectPush: (qid: string) => call<void>('reject_push', { qid }),
  // 发布目标（owner-followups-1015 #14）：设置·发布页；凭据明文只经 setReleaseCredential 进钥匙串，读面只回「已存入」。
  releaseTargets: () => call<ReleaseTarget[]>('release_targets'),
  saveReleaseTarget: (draft: ReleaseTargetDraft) => call<ReleaseTarget>('save_release_target', { draft }),
  removeReleaseTarget: (id: string) => call<void>('remove_release_target', { id }),
  setReleaseCredential: (id: string, credential: string, secret: string) =>
    call<ReleaseTarget>('set_release_credential', { id, credential, secret }),
  removeReleaseCredential: (id: string, credential: string) =>
    call<ReleaseTarget>('remove_release_credential', { id, credential }),
  // 发布执行（owner-followups-1015 #15）：发起只入队发布确认卡（必问、永不记忆）；确认后宿主跑登记的命令。
  requestRelease: (targetId: string) => call<string>('request_release', { targetId }),
  confirmRelease: (qid: string) => call<ReleaseOutcome>('confirm_release', { qid }),
  rejectRelease: (qid: string) => call<void>('reject_release', { qid }),
  // 登记发布目标卡（owner-followups-1015 #16）：Agent 在聊天里拟的目标，负责人批准才写入登记表。
  approveReleaseTargetCard: (qid: string) => call<ReleaseTarget>('approve_release_target_card', { qid }),
  rejectReleaseTargetCard: (qid: string) => call<void>('reject_release_target_card', { qid }),
  // ---- 崩溃恢复（票 37）----
  recoverRun: (runId: string) => call<void>('recover_run', { runId }),
  reconcileToolAction: (actionId: string) => call<ReconcileReport>('reconcile_tool_action', { actionId }),
  offerContinueAfterUnknown: (actionId: string) => call<boolean>('offer_continue_after_unknown', { actionId }),
  abandonToolAction: (actionId: string, reason: string, expectedProjectRoot: string) => call<void>('abandon_tool_action', { actionId, reason, expectedProjectRoot }),
  retryToolAction: (actionId: string, reason: string, acceptsDuplicate: boolean) => call<void>('retry_tool_action', { actionId, reason, acceptsDuplicate }),
  resumeToolAction: (actionId: string) => call<void>('resume_tool_action', { actionId }),
  // ---- 失速卡（stall-watch 票 02/04）：再试一次 / 知道了 ----
  stallRetry: (questionId: string) => call<void>('stall_retry', { questionId }),
  stallAck: (questionId: string) => call<void>('stall_ack', { questionId }),
  // ---- 检验覆盖（票 40）：显式覆盖留痕，composer /override <理由> 同权 ----
  overrideChecks: (reason: string) => call<OverrideOutcome>('override_checks', { reason }),
  // ---- 安装助手（票 36）：NL 请求 → 确认卡 → 负责人确认才执行；grants 永不动 ----
  // ui-audit-2 票 08 裁决：request_install IPC 已删——composer `/install <描述>`
  // 走 TextCommand::Install 同函数直达，owner 发起面就是 composer。
  resolveInstall: (qid: string, allow: boolean) =>
    call<InstallOutcome>('resolve_install', { qid, allow }),
  // ---- 角色编辑（票 30）：项目覆盖行 + 实例字段 + 授权名单（人手编辑面，非提案）----
  agentDetail: (agentId: string) => call<AgentDetail>('agent_detail', { agentId }),
  updateAgent: (agentId: string, patch: AgentPatch) =>
    call<void>('update_agent', { agentId, patch }),
  createRole: (def: RoleDef) => call<string>('create_role', { def }),
  setAgentGrants: (agentId: string, kind: string, names: string[]) =>
    call<void>('set_agent_grants', { agentId, kind, names }),
  // 票 04：授权确认。L4 由核自动写入当前项目；L0–L3 出卡后 confirmGrant。
  requestGrant: (agentId: string, kind: string, name: string) =>
    call<GrantOutcome>('request_grant', { agentId, kind, name }),
  confirmGrant: (qid: string, allow: boolean) =>
    call<GrantOutcome>('confirm_grant', { qid, allow }),
  draftRoleDef: (agentId: string, hint: string) =>
    call<string>('draft_role_def', { agentId, hint }),
  // ---- 流程包编辑（票 31）：draft=pack.json / active=钉住副本，编辑只碰 draft ----
  packDraft: () => call<PackDef>('pack_draft'),
  currentProcessPack: () => call<PackDef>('current_process_pack'),
  savePackDraft: (packJson: string) => call<void>('save_pack_draft', { packJson }),
  savePackTemplate: (packJson: string) => call<string>('save_pack_template', { packJson }),
  packTemplates: () => call<string[]>('pack_templates'),
  // ui-audit-2 票 08：按名载入模板到编辑器
  packTemplate: (name: string) => call<PackDef>('pack_template', { name }),
  exportPackYaml: (dest: string) => call<void>('export_pack_yaml', { dest }),
  exportEvents: (args: { path: string; stageRunId?: string; agentId?: string; kinds?: string[] }) =>
    call<number>('export_events', {
      path: args.path,
      stageRunId: args.stageRunId ?? null,
      agentId: args.agentId ?? null,
      kinds: args.kinds ?? null,
    }),
  // ---- 头像（票 17：角色层 + 实例覆盖；读 agentAvatar 得生效图）----
  setAgentAvatar: (agentId: string, dataUrl: string) =>
    call<void>('set_agent_avatar', { agentId, dataUrl }),
  clearAgentAvatar: (agentId: string) => call<void>('clear_agent_avatar', { agentId }),
  agentAvatar: (agentId: string) => call<string | null>('agent_avatar', { agentId }),
  roleAvatar: (role: string) => call<string | null>('role_avatar', { role }),
  setRoleAvatar: (role: string, dataUrl: string) => call<void>('set_role_avatar', { role, dataUrl }),
  clearRoleAvatar: (role: string) => call<void>('clear_role_avatar', { role }),
  // ---- 画师头像提案（票 20；不是待决卡，不走裁决回路）----
  artistProposal: (id: string) => call<ArtistProposal>('artist_proposal', { id }),
  artistRegenerate: (id: string, role: string) => call<ArtistProposal>('artist_regenerate', { id, role }),
  artistApply: (id: string, roles: string[]) => call<ArtistProposal>('artist_apply', { id, roles }),
  // ---- 启动页 / 最近项目（票 29）----
  recentProjects: () => call<RecentProject[]>('recent_projects'),
  openRecent: (dir: string) => switchProject(source => call<void>('open_recent', { dir, _project: source })),
  removeRecent: (dir: string) => call<void>('remove_recent', { dir }),
  closeProject: () => switchProject(source => call<void>('close_project', { _project: source }), true),
  // ---- 快速通道（票 26）----
  projectInfo: () => call<ProjectInfo>('project_info'),
  dispatch: (role: string, input: string, attachments: AttachRef[] = []) =>
    call<TurnOutcome>('dispatch', { role, input, attachments }),
  upgradeToPack: (packName: string) => call<void>('upgrade_to_pack', { packName }),
  // ---- 项目向导（票 24）----
  projectOpen: () => call<boolean>('project_open'),
  inspectDir: (dir: string) => call<DirReport>('inspect_dir', { dir }),
  presetRoles: () => call<RoleDef[]>('preset_roles'),
  presetPacks: () => call<PackDef[]>('preset_packs'),
  // ---- 角色模板库（ADR 0057：内置∪~/.hexagon/roles.json，无项目可用）----
  listRoleTemplates: () => call<RoleTemplate[]>('list_role_templates'),
  saveRoleTemplate: (def: RoleDef) => call<void>('save_role_template', { def }),
  deleteRoleTemplate: (name: string) => call<void>('delete_role_template', { name }),
  /// UI 内部路由：请求打开共享设置页（启动页齿轮 / 向导 keys 步 / 后续任意入口）。
  /// 不走 IPC——广播 hexagon:open-settings，App/Launcher 各自挂监听。
  openSettings: () => {
    window.dispatchEvent(new CustomEvent('hexagon:open-settings'))
    return Promise.resolve()
  },
  // ui-audit-2 票 08 裁决：check_model_keys/set_model_key 已删——槽位级
  // model/{slot} 凭据被 provider/{id} 供应商级 key 取代（后端同步删命令）。
  // ---- 供应商配置（设置页模型区 / 启动页设置共用；ProviderDoc=供应商+槽位绑定）----
  listProviders: () => call<ProvidersView>('list_providers'),
  saveProvider: (provider: Partial<ProviderDef>, secret?: string) =>
    call<void>('save_provider', { provider, secret: secret ?? null }),
  deleteProvider: (id: string) => call<void>('delete_provider', { id }),
  setSlotBinding: (slot: string, providerId: string, model: string) =>
    call<void>('set_slot_binding', { slot, providerId, model }),
  removeSlotBinding: (slot: string) => call<void>('remove_slot_binding', { slot }),
  fetchProviderModels: (id: string) => call<ModelEntry[]>('fetch_provider_models', { id }),
  // gap B4：向导服务商步实时密钥检测（请求一次模型目录；secret 空 = 用已存钥匙）
  probeProviderKey: (provider: ProviderDef, secret: string | undefined, model: string) =>
    call<KeyProbe>('probe_provider_key', { provider, secret: secret || null, model }),
  // gap B8：内置服务商预设（向导服务商下拉；静态表，读组）
  providerPresets: () => call<ProviderPreset[]>('provider_presets'),
  /** 票 21：表单「实际请求地址」预览（core provider_url，与保存同一道规范化）；null = 不预览。 */
  providerRequestUrl: (kind: ProviderView['kind'], baseUrl: string) =>
    call<string | null>('provider_request_url', { kind, baseUrl }),
  agentsMdDraft: (name: string) => call<string>('agents_md_draft', { name }),
  /// 票 16：空目录的一句话 → 项目说明草稿。不写磁盘。
  /// 2026-09-25 答问优化流：qa 携带答问卡收集的答案（可选）。
  optimizeAgentsMd: (name: string, sentence: string, qa?: BriefQA[]) =>
    call<string>('optimize_agents_md', { name, sentence, qa: qa ?? null }),
  /// 起草前先出答问题目（题数不设上限，每题 2–4 选项）；解析失败/空 → 界面回落直出。
  briefQuestions: (name: string, sentence: string) =>
    call<BriefQuestion[]>('brief_questions', { name, sentence }),
  /// 按项目说明批量起草勾选角色的职责段落（只回草稿；ADR 0075 起不产 globs）。
  draftRoleDefs: (brief: string, roles: RoleDef[]) =>
    call<RoleSeedDraft[]>('draft_role_defs', { brief, roles }),
  draftFlow: (sentence: string, roles: string[]) => call<PackDef>('draft_flow', { sentence, roles }),
  readInstructionFile: (dir: string) => call<string>('read_instruction_file', { dir }),
  draftRoleDuty: (name: string, hint: string) =>
    call<string>('draft_role_duty', { name, hint }),
  /// 票 17：进工作台后的只读开场分析。不挡住输入；空目录和再次打开是空操作。
  runOpeningIntake: () => call<void>('run_opening_intake'),
  confirmIntakeBrief: () => call<void>('confirm_intake_brief'),
  intakeDraftPending: () => call<boolean>('intake_draft_pending'),
  // gap B1：草案卡 inset 预览 / 编辑后整份写回 / 「先不写」持久丢弃
  intakeDraft: () => call<string | null>('intake_draft'),
  updateIntakeDraft: (body: string) => call<void>('update_intake_draft', { body }),
  discardIntakeDraft: () => call<void>('discard_intake_draft'),
  // gap B10：「已丢弃 · 撤销」——把刚丢弃的草案挪回待点头（不落盘）
  restoreIntakeDraft: () => call<void>('restore_intake_draft'),
  /// 票 14：`onStep` 在每步真正结束时被调用（Tauri Channel），不是定时器。
  /// 命令拒绝 = 没打开；调用方不得在拒绝之后进入工作台。
  /// 票 01：`autonomy` 缺省由核落 L4。非法档位核拒绝且不建项目。
  createProject: (
    opts: {
      dir: string
      name: string
      roles: string[]
      /// ADR 0057：选中角色的生效定义全集（自定义模板 + 向导定制项都经此传入，
      /// 后端 override 优先、回落内置；不传=纯内置目录）。
      roleOverrides?: RoleDef[]
      packName?: string | null
      pack?: PackDef | null
      fastpathRole?: string | null
      initGit: boolean
      agentsMd?: string | null
      autonomy?: string | null
      /// 票 17：向导角色详情换的头像（data URL），建项目时按角色写入。
      roleAvatars?: { role: string; data_url: string }[]
    },
    onStep?: (step: CreateStep) => void,
  ) => {
    const args = {
      opts: {
        dir: opts.dir,
        name: opts.name,
        roles: opts.roles,
        roleOverrides: opts.roleOverrides ?? null,
        packName: opts.packName ?? null,
        pack: opts.pack ?? null,
        fastpathRole: opts.fastpathRole ?? null,
        initGit: opts.initGit,
        agentsMd: opts.agentsMd ?? null,
        autonomy: opts.autonomy ?? null,
        roleAvatars: opts.roleAvatars ?? null,
      },
    }
    if (!isTauri) {
      // 浏览器 dev 没有壳层通道。按核的顺序当场报告再返回，不另睡假装耗时。
      for (const step of CREATE_STEPS) onStep?.(step)
      return switchProject(source => call<void>('create_project', { ...args, _project: source }))
    }
    const onProgress = new Channel<CreateStep>()
    onProgress.onmessage = (step) => onStep?.(step)
    return switchProject(source => call<void>('create_project', { ...args, onProgress, _project: source }))
  },
}

// ---- 浏览器 dev mock：覆盖全部事件 kind / 全部待决卡 / 产物版本链 / 代码文件 ----
// a1（架构师）是「完整 agent」样板：回合、工具组、消息、交付、权限、复审、打回、提案全有。

// Browser preview only: a running session with one capability per kind so
// the recovery strip renders without the native host.
const MOCK_SESSION_SUMMARY: SessionSummary = {
  session: {
    session_id: 'session-mock', project_id: 'p1', project_root: '/mock/project', baseline: 'main', status: 'running',
    current_turn_id: '512', worktree: null, worktree_intent: null, runtime: null, reconnect_attempts: 0, reconnect_budget: 3,
    is_current: true, created_at: '', updated_at: '', last_branch: 'session_transition', last_reason_code: 'session_started', last_elapsed_ms: 0,
  },
  recovery_run_id: null, pending_count: 2, highest_pending_severity: 'critical', recent_diagnostics: [],
  capabilities: [
    { kind: 'browser', id: 'browser', available: true, granted: false, health: 'healthy', last_error: null, requires_user_action: false, binding_hint: 'project:/mock/project' },
    { kind: 'computer_use', id: 'desktop', available: true, granted: false, health: 'unknown', last_error: null, requires_user_action: false, binding_hint: 'project:/mock/project' },
    { kind: 'provider', id: 'default', available: true, granted: false, health: 'healthy', last_error: null, requires_user_action: false, binding_hint: 'default' },
    { kind: 'mcp', id: 'docs', available: true, granted: false, health: 'unavailable', last_error: 'mcp_worker_exited', requires_user_action: true, binding_hint: 'project:/mock/project' },
  ],
  triggers: [], next_action: null,
}

// 票 16c：`?mocksession` 让 SessionHost 黄金路径在浏览器 dev 里可走——暂停→继续、
// 请求工作树→坞内工作树确认卡→确认后挂上、稍后继续预约。不带标志时保持静态样例（vitest 依赖）。
// #22：dev 夹具的预设补齐 core ProviderPreset 的说明性字段（手工条目默认无图标 / 无示例）。
const mockPreset = (p: Partial<ProviderPreset> & Pick<ProviderPreset, 'id' | 'name' | 'kind' | 'base_url'>): ProviderPreset => ({
  default_model: '', icon_key: '', website: '', docs: '', sample_models: [], origin: 'manual', ...p,
})

const mockSessionOn = () => typeof location !== 'undefined' && new URLSearchParams(location.search).has('mocksession')
const MOCK_SESSION = {
  status: 'running' as SessionRecord['status'], reason: 'session_started',
  worktree: null as SessionRecord['worktree'],
  card: null as null | { id: string; path: string },
  triggers: [] as SessionSummary['triggers'],
}
const mockSessionSummary = (): SessionSummary => ({
  ...MOCK_SESSION_SUMMARY,
  session: { ...(MOCK_SESSION_SUMMARY.session as SessionRecord), status: MOCK_SESSION.status, last_reason_code: MOCK_SESSION.reason, worktree: MOCK_SESSION.worktree },
  triggers: MOCK_SESSION.triggers,
})
const mockWorktreeCard = () => MOCK_SESSION.card
  ? [{ id: MOCK_SESSION.card.id, kind: 'worktree', agent_id: null, payload: { operation: 'create', path: MOCK_SESSION.card.path, baseline: 'main', baseline_commit: '3a7bca5e9f01', session_id: 'session-mock' }, state: 'queued' }]
  : []
function mockSessionCmd(cmd: string, args?: Record<string, unknown>): boolean {
  if (!mockSessionOn()) return false
  switch (cmd) {
    case 'session_pause': MOCK_SESSION.status = 'paused'; MOCK_SESSION.reason = 'owner_paused'; return true
    case 'session_resume': MOCK_SESSION.status = 'running'; MOCK_SESSION.reason = 'owner_resumed'; return true
    case 'session_request_worktree': MOCK_SESSION.card = { id: 'q-tree', path: String(args?.path ?? '/mock/project-worktrees/session') }; return true
    case 'confirm_worktree':
      if (MOCK_SESSION.card) MOCK_SESSION.worktree = { path: MOCK_SESSION.card.path, baseline: 'main', baseline_commit: '3a7bca5e9f01', created_by_session: true, cleanup_requested: false }
      MOCK_SESSION.card = null; MOCK_SESSION.reason = 'worktree_attached'; return true
    case 'reject_worktree': MOCK_SESSION.card = null; return true
    case 'session_schedule_once':
      MOCK_SESSION.triggers = [{ trigger_id: String(args?.triggerId ?? args?.trigger_id ?? 't1'), session_id: 'session-mock', project_root: '/mock/project', baseline: 'main', model_slot: 'default', permission_package: [], fire_at: Number(args?.fireAt ?? args?.fire_at ?? 0), trace_id: 'mock', state: 'scheduled', created_at: '', consumed_at: null }]
      MOCK_SESSION.reason = 'trigger_scheduled'; return true
    case 'session_cancel_trigger':
      MOCK_SESSION.triggers = MOCK_SESSION.triggers.map((t) => t.trigger_id === String(args?.triggerId) && t.state === 'scheduled' ? { ...t, state: 'cancelled' as const } : t)
      MOCK_SESSION.reason = 'trigger_cancelled'; return true
    default: return false
  }
}

const mockAvatars: Record<string, string> = {
  // a1 预置自定义头像，走通 agentAvatar data URL 通路
  a1: `data:image/svg+xml;utf8,${encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">' +
    '<rect width="64" height="64" rx="16" fill="#1d2634"/>' +
    '<circle cx="32" cy="26" r="11" fill="#e8a33d"/>' +
    '<path d="M14 58c3-11 10-16 18-16s15 5 18 16z" fill="#e8a33d"/>' +
    '</svg>',
  )}`,
}

/// 票 17：角色层 mock（按角色名），生效图 = 实例覆盖 > 角色图。
const mockRoleAvatars: Record<string, string> = {}
const mockEffective = (id: string, role?: string): string | undefined =>
  mockAvatars[id] ?? (role ? mockRoleAvatars[role] : undefined)
// 票 20 dev mock：画师头像提案。图是本地拼的 SVG 色块（不出网），第二格模拟一次出图失败。
const mockArtistProposals: Record<string, ArtistProposal> = {}
const mockArtistImage = (role: string, rev: number) => {
  const hue = ([...role].reduce((h, c) => h + c.charCodeAt(0), 0) * 37 + rev * 90) % 360
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="hsl(${hue} 60% 70%)"/><rect x="2" y="2" width="4" height="4" fill="hsl(${hue} 60% 35%)"/></svg>`
  return `data:image/svg+xml;base64,${btoa(svg)}`
}
const mockArtistProposal = (id: string): ArtistProposal => {
  if (!mockArtistProposals[id]) {
    const roles = (mock('team') as { role: string }[]).map((r) => r.role)
    mockArtistProposals[id] = {
      id, style: 'pixel art', model: 'gpt-image-1', applied: [],
      items: roles.map((role, i) => ({
        role,
        prompt: `pixel art. A square avatar portrait representing the team role "${role}". One centered subject, simple background, no text, no letters, no watermark.`,
        image: i === 1 ? null : mockArtistImage(role, 0),
        failure: i === 1 ? 'provider_unavailable' : null,
      })),
    }
  }
  return mockArtistProposals[id]
}

const T0 = '2026-09-18T'

/// mock 头像哈希（票 07）：FNV-1a，对 mockAvatars 内容敏感——
/// set_agent_avatar 后 team 行哈希必须变，store 才会重拉。
const mockHash = (s: string): string => {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return (h >>> 0).toString(16)
}

type Payload = Record<string, unknown>
const mkEv = (
  id: number, kind: EventKind, agent_id: string | null, stage_run_id: string | null,
  payload: Payload, hhmm: string,
): TimelineItem => ({
  event: { id, project_id: 'p1', kind, agent_id, stage_run_id, payload, created_at: `${T0}${hhmm}:00Z` },
  message: null,
})
const mkMsg = (
  id: number, kind: EventKind, agent_id: string | null, stage_run_id: string | null,
  author: string, body: string, hhmm: string,
): TimelineItem => ({
  event: { id, project_id: 'p1', kind, agent_id, stage_run_id, payload: {}, created_at: `${T0}${hhmm}:00Z` },
  // gap B9：Agent 回复带发出时的模型；负责人消息无模型
  message: { id, author, body, tokens: [], attachments: [], element_refs: [], model: author === 'owner' ? null : 'deepseek/deepseek-chat', created_at: `${T0}${hhmm}:00Z`, thinking: '' },
})

const ART_CONTENT: Record<string, Record<number, string>> = {
  'docs/impl-notes.md': {
    1: '# 实现要点\n\n- 列表筛选走 visible 字段\n- 计划页落 src/plan.ts\n- recipes.test.ts 覆盖筛选分支\n',
  },
  'specs/prd.md': {
    1: '# 食谱 App 产品规格 v1\n\n## 目标\n家庭一周菜谱规划。\n\n## 范围\n- 食谱列表\n- 周计划\n\n## 验收\n1. 能建/改食谱\n2. 能排一周\n',
    2: '# 食谱 App 产品规格 v2\n\n## 目标\n家庭一周菜谱规划，支持中途替换。\n\n## 范围\n- 食谱列表（标签筛选）\n- 周计划（含「换一道」）\n- 详情页\n\n## 验收\n1. 能建/改食谱\n2. 能排一周\n3. 周计划内可替换某道菜\n',
  },
  'docs/arch.md': {
    1: '# 结构说明 v1\n\n单仓两层：`ui/` 视图壳 + `crates/hexagon-core` 业务核。\n\n- 唯一接缝：进程内 API（命令 + 事件推送）\n- 事件表是唯一真相层，群聊只是投影\n- 产物落 `.hexagon/`，版本链 v1→vN\n',
  },
  'ui/screens.md': {
    1: '# 界面稿 v1\n\n三屏：食谱列表 / 详情 / 周计划。\n\n列表：卡片网格 + 搜索框。\n详情：食材 + 步骤。\n周计划：7 列日历格。\n',
    2: '# 界面稿 v2\n\n三屏：食谱列表 / 详情 / 周计划。\n\n列表：卡片网格 + 搜索框 + 标签 chip。\n详情：食材 + 步骤 + 「加入周计划」。\n周计划：7 列日历格，每格带「换一道」入口（验收 3）。\n',
  },
  'api/spec.md': {
    1: '# 接口说明 v1\n\n## 资源\nrecipe\n\n## 端点\n- GET /api/recipes — 列表（offset 分页）\n- POST /api/recipes — 新建\n\n## 错误码\n400 / 404 / 500\n',
    2: '# 接口说明 v2\n\n## 资源\nrecipe / weekly_plan\n\n## 端点\n- GET /api/recipes — 列表（cursor 分页，?tag= 筛选）\n- POST /api/recipes — 新建\n- GET /api/recipes/:id — 详情\n- PUT /api/weekly-plan/:day — 排某天\n\n## 错误码\n400 / 404 / 409（计划冲突）/ 500\n',
  },
  'src/recipes.ts': {
    1: `// 食谱列表页：拉取、筛选、标签 chip
import { useEffect, useState } from 'react'

export interface Recipe {
  id: string
  title: string
  tags: string[]
  minutes: number
}

export function RecipeList() {
  const [items, setItems] = useState<Recipe[]>([])
  const [q, setQ] = useState('')
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let live = true
    fetch('/api/recipes')
      .then((r) => r.json())
      .then((data: Recipe[]) => { if (live) setItems(data) })
      .finally(() => { if (live) setLoading(false) })
    return () => { live = false }
  }, [])

  const shown = items.filter((r) => r.title.includes(q))

  if (loading) return <p className="dim">加载中…</p>
  return (
    <section>
      <input
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="搜索食谱"
      />
      <ul>
        {shown.map((r) => (
          <li key={r.id}>
            <strong>{r.title}</strong>
            <span className="dim"> · {r.minutes} 分钟</span>
            {r.tags.map((tag) => (
              <span key={tag} className="chip">{tag}</span>
            ))}
          </li>
        ))}
      </ul>
    </section>
  )
}
`,
  },
  'crates/core/src/api.rs': {
    1: `//! 核 API 接缝：Tauri 命令只转发，业务全在 hexagon-core。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineItem {
    pub event_id: i64,
    pub kind: String,
    pub agent_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("project not open")]
    NotOpen,
    #[error("stage {0} is not pending")]
    StageNotPending(u32),
    #[error("db: {0}")]
    Db(#[from] rusqlite::Error),
}

/// 时间线：after 之后的事件，按 id 升序。
pub fn timeline(after: Option<i64>, limit: usize) -> Result<Vec<TimelineItem>, ApiError> {
    let rows = crate::db::events_after(after.unwrap_or(0), limit)?;
    Ok(rows.into_iter().map(row_to_item).collect())
}

fn row_to_item(row: crate::db::EventRow) -> TimelineItem {
    TimelineItem {
        event_id: row.id,
        kind: row.kind,
        agent_id: row.agent_id,
        payload: serde_json::from_str(&row.payload).unwrap_or_default(),
    }
}
`,
  },
  'scripts/seed.py': {
    1: `"""开发种子：造一个最小可跑的项目库。"""
import sqlite3
from pathlib import Path

DB = Path(".hexagon/state.db")

ROLES = ["产品策划", "架构师", "前端", "后端", "QA"]


def seed(conn: sqlite3.Connection) -> None:
    conn.execute("DELETE FROM agents")
    for i, role in enumerate(ROLES):
        conn.execute(
            "INSERT INTO agents (id, role, model_slot, status) VALUES (?, ?, ?, ?)",
            (f"a{i}", role, "chat", "active"),
        )
    conn.commit()


if __name__ == "__main__":
    if not DB.exists():
        raise SystemExit(f"missing {DB}")
    with sqlite3.connect(DB) as conn:
        seed(conn)
    print(f"seeded {len(ROLES)} agents")
`,
  },
  'tests/qa-report.md': {
    1: '# 测试记录 · 实现阶段\n\n## 结果\n| 用例 | 命令 | 结果 |\n| --- | --- | --- |\n| 界面冒烟 | `npm run test:ui` | ⚠️ 1 项跳过 |\n\n（缺环境信息，被打回）\n',
    2: '# 测试记录 · 实现阶段\n\n## 环境\n- macOS 15 · cargo 1.85 · node 22\n\n## 结果\n| 用例 | 命令 | 结果 |\n| --- | --- | --- |\n| 事件追加 | `cargo test events` | ✅ 12/12 |\n| 权限管线 | `cargo test permission` | ✅ 8/8 |\n| 界面冒烟 | `npm run test:ui` | ⚠️ 1 项跳过 |\n\n## 结论\n实现产物与接口说明 v2 一致。\n',
  },
  'reviews/be-spec.md': {
    1: '# 复审意见 · api/spec.md v2\n\n复审者：后端技术负责人\n\n## 结论\n通过。\n\n## 意见\n- 错误码表已补齐 409 冲突分支。\n- cursor 分页方向正确，注意与前端对齐游标语义。\n',
  },
  'proposals/p2.md': {
    1: '# 改进提案 p2 · 工具白名单调整\n\n生效面：项目权限基线。\n\n```diff\n  allow:\n    - fs.read\n    - fs.write\n-   - bash:cargo *\n+   - bash:cargo build\n+   - bash:cargo test\n    - sqlite3:.hexagon/*\n```\n',
  },
  'docs/markdown-demo.md': {
    1: [
      '# Markdown 渲染演示',
      '',
      '## 排版',
      '普通段落，**加粗**、*斜体*、~~删除线~~、`inline code` 混排。',
      '',
      '> 引用块：阶段盖章前必须过复审。',
      '> 第二行引用。',
      '',
      '---',
      '',
      '## 列表',
      '- 无序项一',
      '- 无序项二',
      '  - 嵌套子项',
      '',
      '1. 有序一',
      '2. 有序二',
      '',
      '- [x] 已完成任务',
      '- [ ] 未完成任务',
      '',
      '## 表格',
      '| 用例 | 命令 | 结果 |',
      '| :--- | :--- | ---: |',
      '| 事件追加 | `cargo test events` | 12/12 |',
      '| 权限管线 | `cargo test permission` | 8/8 |',
      '',
      '## 代码块',
      '```ts',
      'export function hello(name: string): string {',
      '  return `hello ${name}`',
      '}',
      '```',
      '',
      '无语言标记的块：',
      '',
      '```',
      'plain fence line 1',
      'plain fence line 2',
      '```',
      '',
      '链接示例：[Tauri 文档](https://tauri.app)',
    ].join('\n'),
  },
}

const artLatest = (path: string): string | null => {
  const vs = ART_CONTENT[path]
  if (!vs) return null
  return vs[Math.max(...Object.keys(vs).map(Number))]
}

// 票 04 黄金标准手测：`?mockdecide` 时 mock 记住已裁决的卡，pending_questions 不再返回它们
// （默认关：既有测试依赖静态夹具）。键 = 卡 id；recover_run / stamp 按夹具里对应卡反查。
const MOCK_DECIDED = new Set<string>()
const MOCK_DECIDE_CMDS: Record<string, (a: Record<string, unknown>) => string> = {
  answer_permission: (a) => String(a.questionId), allow_project_permission: (a) => String(a.questionId),
  confirm_push: (a) => String(a.qid), reject_push: (a) => String(a.qid),
  confirm_release: (a) => String(a.qid), reject_release: (a) => String(a.qid),
  approve_release_target_card: (a) => String(a.qid), reject_release_target_card: (a) => String(a.qid),
  confirm_worktree: (a) => String(a.qid), reject_worktree: (a) => String(a.qid),
  adjudicate_flag: (a) => String(a.qid), confirm_proposal: (a) => String(a.qid), reject_proposal: (a) => String(a.qid),
  stall_retry: (a) => String(a.questionId), stall_ack: (a) => String(a.questionId),
  recover_run: () => 'q-rec', stamp: () => 'q-stamp', reject_stamp: () => 'q-stamp',
}

const mockManaged = () => typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockmanaged')
let MOCK_MANAGED = { enabled: true, paused: false }
const MOCK_MANAGED_HISTORY = [
  { event_id: 124, card_id: 'q-m3', card_kind: 'push', action: 'deny', reason: 'target_missing', via: 'host', outcome: 'not_pushed', actor: 'managed:g1:1', result: 'incomplete', scope: 'out_of_scope', evidence: 'host_rule', next: { kind: 'timeline', target: '124' } },
  { event_id: 123, card_id: 'q-m2', card_kind: 'stamp', action: 'accept', reason: 'evidence_complete', via: 'host', outcome: 'stamped', actor: 'managed:g1:1', result: 'done', scope: 'in_scope', evidence: 'real_check', next: { kind: 'timeline', target: '123' } },
  { event_id: 122, card_id: 'q-m1', card_kind: 'permission', action: 'allow', reason: 'model_choice', via: 'model', outcome: 'allowed', actor: 'managed:g1:1', result: 'done', scope: 'in_scope', evidence: 'model_choice', next: { kind: 'timeline', target: '122' } },
]
const MOCK_MANAGED_EXTERNAL = [
  { event_id: 122, card_id: 'q-m1', kind: 'local_trash', target: '~/Downloads/old-build.zip', action: 'move_to_trash', decision: 'allow', result: 'done', at: '2026-10-14 03:58:00', verification: 'host_verified_recoverable', trash_path: '~/.Trash/old-build.zip' },
  { event_id: 123, card_id: 'q-m2', kind: 'mcp', target: 'figma:update_node', action: 'update_node', decision: 'allow', result: 'done', at: '2026-10-14 03:59:00', verification: 'host_unverified_deletion', trash_path: null },
]
const mockIntake: { draft: string | null | undefined; discarded?: string | null } = { draft: undefined }
const mockIntakeDraft = (): string | null => {
  if (mockIntake.draft === undefined) {
    mockIntake.draft = typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockintake')
      ? '## Purpose\n离线可用的待办 PWA，单用户，本地存储。\n## Commands\nnpm run dev · npm test\n## Layout\n未知\n'
      : null
  }
  return mockIntake.draft
}
// gap D6 截图比对（control#action）：`?mockcomputer` 时给一张带理由与截图引用的电脑动作卡，desktop_screenshot 回一张示意图；默认关
const mockComputer = () => typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockcomputer')
const MOCK_COMPUTER_SHOT = `data:image/svg+xml;utf8,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1000"><rect width="1600" height="1000" fill="#c9ced6"/><rect x="160" y="120" width="1280" height="760" rx="18" fill="#f7f7f8"/><rect x="160" y="120" width="1280" height="56" rx="18" fill="#e6e7ea"/><rect x="200" y="220" width="300" height="620" rx="10" fill="#eceef1"/><rect x="560" y="240" width="820" height="40" rx="8" fill="#fff"/><rect x="560" y="300" width="820" height="40" rx="8" fill="#fff"/><rect x="1300" y="306" width="56" height="28" rx="14" fill="#34c759"/></svg>')}`
const mockDesign = () => typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockdesign')
const mockSvg = (a: string, b: string) => `data:image/svg+xml;utf8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="320" height="240"><defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="320" height="240" fill="url(#g)"/></svg>`)}`
const MOCK_DESIGN_OPTIONS = [
  { id: 'crisp', title: '清爽工具', description: 'Inter · 蓝灰 · 8px 圆角', layout: '双栏', typography: 'Inter', palette: '蓝灰', mockups: [{ page: '首页', mime: 'image/svg+xml', digest: 'm1', data_url: mockSvg('#eef4fc', '#d6e4f5') }] },
  { id: 'night', title: '夜间专注', description: '深色优先 · 紫色强调', layout: '单栏', typography: 'Inter', palette: '深色', mockups: [{ page: '首页', mime: 'image/svg+xml', digest: 'm2', data_url: mockSvg('#2a2a33', '#15151b') }] },
  { id: 'warm', title: '温暖手账', description: '衬线标题 · 米色纸感', layout: '卡片', typography: '衬线', palette: '米色', mockups: [{ page: '首页', mime: 'image/svg+xml', digest: 'm3', data_url: mockSvg('#fdf0dc', '#f8d6b8') }] },
]

let MOCK_RELEASE: ReleaseTarget[] = [
  { id: 'rel-npm', name: 'npm', command: 'npm publish --access public', cwd: 'packages/web', credentials: [{ name: 'NPM_TOKEN', set: true }, { name: 'NPM_OTP_SEED', set: false }] },
  { id: 'rel-pages', name: 'GitHub Pages', command: 'npm run deploy', cwd: '', credentials: [] },
]
function mockSaveRelease(draft: ReleaseTargetDraft): ReleaseTarget {
  const prev = MOCK_RELEASE.find((r) => r.id === draft.id)
  const names = [...new Set(draft.credentials.map((c) => c.trim()).filter(Boolean))]
  const next: ReleaseTarget = {
    id: prev?.id ?? `rel-${Date.now()}`, name: draft.name.trim(), command: draft.command.trim(), cwd: draft.cwd.trim(),
    credentials: names.map((name) => ({ name, set: prev?.credentials.find((c) => c.name === name)?.set ?? false })),
  }
  MOCK_RELEASE = prev ? MOCK_RELEASE.map((r) => (r.id === prev.id ? next : r)) : [...MOCK_RELEASE, next]
  return next
}

function mock<T>(cmd: string, args?: Record<string, unknown>): T {
  if (MOCK_DECIDE_CMDS[cmd] && typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockdecide')) {
    MOCK_DECIDED.add(MOCK_DECIDE_CMDS[cmd](args ?? {}))
  }
  if (mockSessionCmd(cmd, args)) return null as T
  return mockReply<T>(cmd, args)
}

function mockReply<T>(cmd: string, args?: Record<string, unknown>): T {
  switch (cmd) {
    case 'core_ping':
      return 'hexagon-core ok' as T
    case 'prompt_catalog':
      return [
        { id: 'workbench.base', group: 'workbench', text: '# Hexagon workbench\nYou are an agent on a Hexagon workbench…', hash: '0' },
        { id: 'tool.fs_patch', group: 'tools', text: 'Replace an exact string in a repo file.', hash: '1' },
      ] as T
    case 'translate_prompts':
      return {
        no_model: false,
        entries: args?.lang === 'en' ? [] : [
          { id: 'workbench.base', text: '（参考译文）Hexagon 工作台…', error: null },
          { id: 'tool.fs_patch', text: '（参考译文）在仓库文件中替换一段精确字符串。', error: null },
        ],
      } as T
    case 'stage_evidence':
      return null as T
    case 'stage_status':
      return [
        { run_id: 'r0', stage: '需求', seq: 0, state: 'done' },
        { run_id: 'r1', stage: '界面稿', seq: 1, state: 'done' },
        { run_id: 'r2', stage: '接口', seq: 2, state: 'done' },
        { run_id: 'r3', stage: '实现', seq: 3, state: 'waiting_stamp' },
        { run_id: 'r4', stage: '灰度', seq: 4, state: 'skipped' },
        { run_id: 'r5', stage: '检验', seq: 5, state: 'pending' },
        { run_id: 'r6', stage: '交付', seq: 6, state: 'pending' },
        { run_id: 'r9', stage: '部署演练', seq: 7, state: 'interrupted' },
      ] as T
    case 'team':
      return [
        { id: 'a0', role: '产品策划', status: 'active', model_slot: 'chat', duty: '把一句话需求展开成规格与验收标准' },
        { id: 'a1', role: '架构师', status: 'active', model_slot: 'chat', duty: '定技术方案、模块边界与接口' },
        { id: 'a2', role: 'UX', status: 'sleeping', model_slot: 'chat', duty: '信息结构与交互流程' },
        { id: 'a3', role: '前端', status: 'active', model_slot: 'code', duty: '实现界面与前端逻辑' },
        { id: 'a4', role: '后端', status: 'active', model_slot: 'code', duty: '实现服务端接口与数据' },
        { id: 'a5', role: 'QA', status: 'active', model_slot: 'chat', duty: '写测试用例并验证交付' },
        { id: 'a6', role: '后端技术负责人', status: 'sleeping', model_slot: 'chat', duty: '复审后端提案' },
        { id: 'a7', role: '运维', status: 'sleeping', model_slot: 'ops', duty: '部署与发布' },
      ].map((r) => ({
        ...r,
        avatar_hash: mockEffective(r.id, r.role) ? mockHash(`${!!mockAvatars[r.id]}:${mockEffective(r.id, r.role)}`) : null,
        avatar_override: !!mockAvatars[r.id],
        // gap B2：团队行带解析后的模型展示名（dev 样例对照 workbench#running 消息头）
        model: ({ chat: 'Claude Sonnet 5.5', code: 'Claude Sonnet 5.5', ops: 'DeepSeek V4' } as Record<string, string>)[r.model_slot] ?? null,
        superior: ({ 后端: '后端技术负责人', 前端: '架构师', UX: '产品策划', QA: '架构师' } as Record<string, string>)[r.role] ?? null,
      })) as T
    case 'artifacts':
      return [
        { id: 'art1', path: 'specs/prd.md', kind: '规格', tier: 'skeleton', stage_run_id: 'r0', author: 'a0', version: 1, status: 'superseded', upstream_id: null },
        { id: 'art2', path: 'specs/prd.md', kind: '规格', tier: 'skeleton', stage_run_id: 'r0', author: 'a0', version: 2, status: 'stamped', upstream_id: null },
        { id: 'art3', path: 'docs/arch.md', kind: '结构说明', tier: 'freeform', stage_run_id: 'r1', author: 'a1', version: 1, status: 'stamped', upstream_id: null },
        { id: 'art4', path: 'ui/screens.md', kind: '界面稿', tier: 'freeform', stage_run_id: 'r1', author: 'a2', version: 1, status: 'superseded', upstream_id: null },
        { id: 'art5', path: 'ui/screens.md', kind: '界面稿', tier: 'freeform', stage_run_id: 'r1', author: 'a2', version: 2, status: 'stamped', upstream_id: null },
        { id: 'art6', path: 'api/spec.md', kind: '接口说明', tier: 'skeleton', stage_run_id: 'r2', author: 'a1', version: 1, status: 'superseded', upstream_id: null },
        { id: 'art7', path: 'api/spec.md', kind: '接口说明', tier: 'skeleton', stage_run_id: 'r2', author: 'a1', version: 2, status: 'stamped', upstream_id: null },
        { id: 'art8', path: 'src/recipes.ts', kind: '代码', tier: 'freeform', stage_run_id: 'r3', author: 'a3', version: 1, status: 'valid', upstream_id: null },
        { id: 'art9', path: 'crates/core/src/api.rs', kind: '代码', tier: 'freeform', stage_run_id: 'r3', author: 'a4', version: 1, status: 'valid', upstream_id: null },
        { id: 'art10', path: 'scripts/seed.py', kind: '代码', tier: 'freeform', stage_run_id: 'r3', author: 'a4', version: 1, status: 'valid', upstream_id: null },
        { id: 'art11', path: 'tests/qa-report.md', kind: '测试记录', tier: 'parse', stage_run_id: 'r3', author: 'a5', version: 1, status: 'superseded', upstream_id: null },
        { id: 'art12', path: 'tests/qa-report.md', kind: '测试记录', tier: 'parse', stage_run_id: 'r3', author: 'a5', version: 2, status: 'pending', upstream_id: null },
        { id: 'art13', path: 'reviews/be-spec.md', kind: '复审意见', tier: 'parse', stage_run_id: 'r2', author: 'a6', version: 1, status: 'valid', upstream_id: null },
        { id: 'art14', path: 'proposals/p2.md', kind: '改进提案', tier: 'parse', stage_run_id: 'r3', author: 'a1', version: 1, status: 'valid', upstream_id: null },
        { id: 'art15', path: 'docs/markdown-demo.md', kind: '结构说明', tier: 'freeform', stage_run_id: 'r1', author: 'a1', version: 1, status: 'valid', upstream_id: null },
        { id: 'art16', path: 'docs/impl-notes.md', kind: '结构说明', tier: 'freeform', stage_run_id: 'r3', author: 'a3', version: 1, status: 'valid', upstream_id: null },
      ] as T
    case 'timeline_window_metadata': {
      const request = args!.request as TimelineWindowMetadataRequest
      const all = mock<TimelineItem[]>('timeline', {limit: Number.MAX_SAFE_INTEGER})
      const pages = request.event_ids.map(event_id => mockTimelineWindow(all, {expected_project_root:request.expected_project_root,filter:'all',agent_id:null,cursor:{kind:'around',event_id},limit:1}))
      return {project_root:request.expected_project_root,watermark:all.at(-1)?.event.id ?? 0,
        boundary_pairs:[...new Map(pages.flatMap(page=>page.boundary_pairs).map(item=>[item.event.id,item])).values()],
        turn_windows:[...new Map(pages.flatMap(page=>page.turn_windows).map(turn=>[turn.start_id,turn])).values()],
        steered_message_ids:[...new Set(pages.flatMap(page=>page.steered_message_ids))]} as T
    }
    case 'timeline_window': return mockTimelineWindow(mock<TimelineItem[]>('timeline', {limit: Number.MAX_SAFE_INTEGER}), args!.request as TimelineWindowRequest) as T
    case 'timeline_facts': return mockTimelineFacts(mock<TimelineItem[]>('timeline', {limit: Number.MAX_SAFE_INTEGER}), args!.request as TimelineFactsRequest) as T
    case 'timeline_nodes': return mockTimelineNodes(mock<TimelineItem[]>('timeline', {limit: Number.MAX_SAFE_INTEGER}), args!.request as TimelineNodesRequest) as T
    case 'timeline': {
      // mock 也尊重 after/limit（票 07 增量通道）：真实后端同语义，
      // dev/测试里才能演练游标归并。
      const all: TimelineItem[] = [
        mkEv(1, 'pack_upgraded', null, null, { pack: '规格驱动' }, '09:00'),
        mkEv(2, 'agent_activated', 'a0', null, {}, '09:00'),
        mkEv(3, 'system', null, null, { note: '快速通道项目钉包副本，切规格驱动 v1' }, '09:00'),

        mkEv(4, 'stage_started', null, 'r0', { stage: '需求', run_id: 'r0' }, '09:01'),
        mkEv(5, 'turn_started', 'a0', 'r0', {}, '09:02'),
        mkEv(6, 'tool_called', 'a0', 'r0', { tool: 'fs_read', input: { path: 'docs/brief.md' }, seq: 'r0:i0' }, '09:02'),
        mkEv(7, 'tool_result', 'a0', 'r0', { tool: 'fs_read', ok: true, result: { output: { content: '# 项目简报\n\n家庭食谱 App，一周菜单规划。' } } }, '09:02'),
        mkEv(8, 'tool_called', 'a0', 'r0', { tool: 'artifact_write', input: { path: 'specs/prd.md', bytes: 486, kind: '规格' }, seq: 'r0:i1' }, '09:03'),
        mkEv(9, 'tool_result', 'a0', 'r0', { tool: 'artifact_write', ok: true, result: { output: { artifact_id: 'art1' } } }, '09:03'),
        mkMsg(10, 'agent_message', 'a0', 'r0', 'a0', '需求梳理完：聚焦家庭一周菜单。规格 v1 落 `specs/prd.md`，验收三条。', '09:05'),
        mkEv(11, 'artifact_delivered', 'a0', 'r0', { path: 'specs/prd.md', kind: '规格', version: 1, status: 'valid' }, '09:05'),
        mkEv(12, 'review_passed', 'a1', 'r0', { path: 'specs/prd.md' }, '09:07'),
        mkEv(13, 'turn_finished', 'a0', 'r0', {}, '09:08'),
        mkEv(14, 'stamped', null, 'r0', { stage: '需求' }, '09:10'),
        mkEv(15, 'stage_finished', null, 'r0', { stage: '需求' }, '09:10'),

        mkEv(16, 'stage_started', null, 'r1', { stage: '界面稿', run_id: 'r1' }, '09:11'),
        mkEv(17, 'agent_activated', 'a2', 'r1', {}, '09:11'),
        mkEv(18, 'turn_started', 'a2', 'r1', {}, '09:12'),
        mkEv(19, 'tool_called', 'a2', 'r1', { tool: 'artifact_write', input: { path: 'ui/screens.md', bytes: 1240, kind: '界面稿' }, seq: 'r1:i0' }, '09:13'),
        mkEv(20, 'tool_result', 'a2', 'r1', { tool: 'artifact_write', ok: true, result: { output: { artifact_id: 'art4' } } }, '09:13'),
        mkMsg(21, 'agent_message', 'a2', 'r1', 'a2', '界面稿 v1 三屏：食谱列表 / 详情 / 周计划。', '09:15'),
        mkEv(22, 'artifact_delivered', 'a2', 'r1', { path: 'ui/screens.md', kind: '界面稿', version: 1, status: 'valid' }, '09:15'),
        mkEv(23, 'flag_submitted', 'a1', 'r1', { flag_id: 'f3', severity: 'high', target: 'ui/screens.md', section: '周计划', reason: '缺「换一道」入口，与验收 3 冲突' }, '09:18'),
        mkEv(24, 'flag_adjudicated', null, 'r1', { flag_id: 'f3', agree: true }, '09:20'),
        mkMsg(25, 'agent_message', 'a2', 'r1', 'a2', '打回成立，补替换入口。v2 已重交。', '09:24'),
        mkEv(26, 'artifact_delivered', 'a2', 'r1', { path: 'ui/screens.md', kind: '界面稿', version: 2, status: 'valid' }, '09:24'),
        mkEv(27, 'review_passed', 'a1', 'r1', { path: 'ui/screens.md' }, '09:26'),
        mkEv(28, 'stamped', null, 'r1', { stage: '界面稿' }, '09:28'),
        mkEv(29, 'stage_finished', null, 'r1', { stage: '界面稿' }, '09:28'),
        mkEv(30, 'agent_slept', 'a2', 'r1', {}, '09:29'),

        mkEv(31, 'stage_started', null, 'r2', { stage: '接口', run_id: 'r2' }, '09:30'),
        mkEv(32, 'consult_wakeup', null, 'r2', { role: '后端技术负责人' }, '09:30'),
        mkEv(33, 'agent_activated', 'a6', 'r2', {}, '09:30'),
        mkEv(34, 'turn_started', 'a1', 'r2', {}, '09:31'),
        mkEv(35, 'tool_called', 'a1', 'r2', { tool: 'fs_read', input: { path: 'specs/prd.md' }, seq: 'r2:i0' }, '09:31'),
        mkEv(36, 'tool_result', 'a1', 'r2', { tool: 'fs_read', ok: true, result: { output: { content: '# 食谱 App 产品规格 v2\n…' } } }, '09:31'),
        mkEv(37, 'tool_called', 'a1', 'r2', { tool: 'artifact_write', input: { path: 'api/spec.md', bytes: 980, kind: '接口说明' }, seq: 'r2:i1' }, '09:32'),
        mkEv(38, 'tool_result', 'a1', 'r2', { tool: 'artifact_write', ok: true, result: { output: { artifact_id: 'art6' } } }, '09:32'),
        mkEv(39, 'permission_asked', 'a1', 'r2', { tool: 'bash', input: { cmd: 'sqlite3 .hexagon/state.db .schema' }, reason: '核对事件表结构' }, '09:33'),
        mkEv(40, 'permission_allowed', null, 'r2', { tool: 'bash', by: 'owner' }, '09:34'),
        mkEv(41, 'permission_shape_remembered', null, null, { shape: 'sqlite3 .hexagon/* .schema' }, '09:34'),
        mkMsg(42, 'agent_message', 'a1', 'r2', 'a1', '接口说明 v1 出了：\n\n- `GET/POST /api/recipes`\n- `GET /api/recipes/:id`\n- 分页暂用 offset\n\n错误码表初版随稿。', '09:36'),
        mkEv(43, 'artifact_delivered', 'a1', 'r2', { path: 'api/spec.md', kind: '接口说明', version: 1, status: 'valid' }, '09:36'),
        mkEv(44, 'review_rejected', 'a6', 'r2', { path: 'api/spec.md', reason: '错误码缺 409；offset 分页不建议' }, '09:40'),
        mkMsg(45, 'agent_message', 'a1', 'r2', 'a1', '复审驳回成立：v2 补 409，分页改 cursor。', '09:45'),
        mkEv(46, 'artifact_delivered', 'a1', 'r2', { path: 'api/spec.md', kind: '接口说明', version: 2, status: 'valid' }, '09:45'),
        mkEv(47, 'review_passed', 'a6', 'r2', { path: 'api/spec.md' }, '09:48'),
        mkEv(48, 'stamped', null, 'r2', { stage: '接口' }, '09:50'),
        mkEv(49, 'stage_finished', null, 'r2', { stage: '接口' }, '09:50'),
        mkEv(50, 'agent_slept', 'a6', 'r2', {}, '09:51'),

        mkEv(51, 'stage_started', null, 'r3', { stage: '实现', run_id: 'r3' }, '09:52'),
        mkEv(52, 'paused', null, null, { by: 'owner' }, '09:55'),
        mkEv(53, 'resumed', null, null, {}, '10:02'),
        mkEv(54, 'owner_command', null, null, { text: '/advance' }, '10:02'),
        mkEv(55, 'turn_started', 'a3', 'r3', {}, '10:03'),
        mkEv(56, 'tool_called', 'a3', 'r3', { tool: 'fs_write', input: { path: 'src/recipes.ts', bytes: 3120 }, seq: 'r3:i0' }, '10:04'),
        mkEv(57, 'tool_result', 'a3', 'r3', { tool: 'fs_write', ok: true, result: { output: { written: 'src/recipes.ts' } } }, '10:04'),
        mkEv(58, 'tool_called', 'a3', 'r3', { tool: 'bash', input: { cmd: 'npm run dev' }, seq: 'r3:i1' }, '10:05'),
        mkEv(59, 'tool_result', 'a3', 'r3', { tool: 'bash', ok: true, result: { output: { exit_code: 0, timed_out: false, stdout: 'VITE v6.1 ready in 318 ms\n\n  ➜  Local:   http://localhost:1420/\n  ➜  Network: use --host to expose', stderr: '' } } }, '10:05'),
        mkEv(60, 'artifact_delivered', 'a3', 'r3', { path: 'src/recipes.ts', kind: '代码', version: 1, status: 'valid' }, '10:07'),
        mkMsg(61, 'agent_message', 'a3', 'r3', 'a3', '列表页能跑了：筛选、标签 chip、加载态。', '10:07'),
        mkEv(62, 'turn_finished', 'a3', 'r3', {}, '10:08'),
        mkEv(63, 'turn_started', 'a4', 'r3', {}, '10:10'),
        mkEv(64, 'tool_called', 'a4', 'r3', { tool: 'fs_write', input: { path: 'crates/core/src/api.rs', bytes: 5210 }, seq: 'r3:i2' }, '10:11'),
        mkEv(65, 'tool_result', 'a4', 'r3', { tool: 'fs_write', ok: true, result: { output: { written: 'crates/core/src/api.rs' } } }, '10:11'),
        mkEv(66, 'tool_called', 'a4', 'r3', { tool: 'bash', input: { cmd: 'cargo build' }, seq: 'r3:i3' }, '10:12'),
        mkEv(67, 'tool_result', 'a4', 'r3', { tool: 'bash', ok: true, result: { output: { exit_code: 101, timed_out: false, stdout: '', stderr: 'error[E0308]: mismatched types\n  --> crates/core/src/api.rs:214:9\n\nerror: could not compile `hexagon-core`' } } }, '10:12'),
        mkEv(68, 'artifact_delivered', 'a4', 'r3', { path: 'crates/core/src/api.rs', kind: '代码', version: 1, status: 'valid' }, '10:14'),
        mkMsg(69, 'agent_message', 'a4', 'r3', 'a4', '核 API 接缝落完：`timeline()` + 事件行投影。', '10:14'),
        mkMsg(70, 'owner_message', null, null, 'owner', '先补列表页单测，再推联调。', '10:16'),
        mkEv(71, 'turn_failed', 'a4', 'r3', { error: '模型供应商超时，已自动重试' }, '10:20'),
        mkEv(72, 'permission_asked', 'a4', 'r3', { tool: 'bash', input: { cmd: 'rm -rf node_modules && npm i' }, reason: '依赖树损坏需重装' }, '10:22'),
        mkEv(73, 'permission_denied', null, 'r3', { tool: 'bash', by: 'owner' }, '10:23'),
        mkEv(74, 'test_ran', 'a5', 'r3', { command: 'npm test', result: 'passed', passed: 24, total: 26 }, '10:25'),
        mkEv(75, 'artifact_delivered', 'a5', 'r3', { path: 'tests/qa-report.md', kind: '测试记录', version: 1, status: 'pending' }, '10:26'),
        mkEv(76, 'artifact_rejected', null, 'r3', { path: 'tests/qa-report.md', version: 1, reason: '缺环境信息' }, '10:27'),
        mkEv(77, 'artifact_delivered', 'a5', 'r3', { path: 'tests/qa-report.md', kind: '测试记录', version: 2, status: 'pending' }, '10:30'),
        mkEv(78, 'review_skipped', null, 'r3', { kind: '测试记录', by: 'owner' }, '10:31'),
        mkEv(79, 'backfill_executed', null, null, { from: 'api/spec.md', to: 'src/recipes.ts' }, '10:33'),
        mkEv(80, 'flag_submitted', 'a4', 'r3', { flag_id: 'f12', severity: 'medium', target: 'api/spec.md', section: '分页', reason: 'cursor 分页与前端 offset 假设冲突' }, '10:35'),
        mkEv(81, 'escalated', null, 'r3', { flag_id: 'f12', target: 'api/spec.md' }, '10:36'),
        mkEv(82, 'proposal_queued', 'a1', 'r3', { proposal_id: 'p0', surface: '阶段顺序' }, '10:38'),
        mkEv(83, 'proposal_rejected', null, 'r3', { proposal_id: 'p0', reason: '与当前节奏冲突' }, '10:40'),
        mkEv(84, 'proposal_queued', 'a1', 'r3', { proposal_id: 'p1', surface: '阶段顺序' }, '10:42'),
        mkEv(85, 'proposal_reviewed', 'a6', 'r3', { proposal_id: 'p1', pass: true }, '10:44'),
        mkEv(86, 'proposal_stamped', null, 'r3', { proposal_id: 'p1' }, '10:45'),
        mkEv(87, 'proposal_activated', null, 'r3', { proposal_id: 'p1' }, '10:45'),
        mkEv(88, 'proposal_rolled_back', null, 'r3', { proposal_id: 'p1', reason: '试运行后顺序更差，回滚' }, '10:55'),
        mkEv(89, 'stage_skipped', null, 'r4', { stage: '灰度' }, '11:00'),
        mkEv(90, 'stage_rewound', null, 'r3', { stage: '实现', to_seq: 2 }, '11:02'),
        mkEv(91, 'autonomy_changed', null, null, { from: 'L0', to: 'L1' }, '11:05'),
        mkEv(92, 'push_requested', null, null, { remote: 'origin', baseline: 'main' }, '11:08'),
        mkEv(93, 'push_failed', null, null, { remote: 'origin', error: '网络超时' }, '11:09'),
        mkEv(94, 'push_requested', null, null, { remote: 'origin', baseline: 'main' }, '11:15'),
        mkEv(95, 'push_confirmed', null, null, { remote: 'origin', baseline: 'main' }, '11:16'),
        mkEv(96, 'baseline_merged', null, null, { baseline: 'main', commit: 'c4f9a2e' }, '11:17'),
        mkEv(97, 'push_requested', null, null, { remote: 'upstream', baseline: 'main' }, '11:20'),
        mkEv(98, 'push_rejected', null, null, { remote: 'upstream', by: 'owner' }, '11:21'),
        mkEv(99, 'usage_cap_hit', null, null, { spent_mc: 40200, limit_cents: 20000 }, '11:25'),
        mkEv(100, 'team_slept', null, null, {}, '11:25'),
        mkEv(101, 'agent_slept', 'a3', null, {}, '11:25'),
        mkMsg(102, 'agent_message', 'a1', 'r3', 'a1', '今日收口：实现待盖章，`f12` 已升级待裁决，origin/main 已发布。', '11:30'),
        mkEv(103, 'fastpath_dispatched', null, null, { role: '运维', input: '准备部署清单' }, '11:32'),
        mkEv(104, 'return_summary', null, null, {
          since_event: 66, deliveries: [], reviews: { passed: 2, rejected: 1 },
          flags: { submitted: 2, adjudicated: 1, escalated: 1 },
          permissions: { asked: 1, allowed: 0, denied: 0 },
          stages: { finished: 2, skipped: 1, rewound: 1 }, pending_todos: [],
          attention: { unresolved_actions: 0, unknown_cost_records: 0, budget_stops: 0, exception_decisions: 0, policy_candidates: 0 },
        } satisfies ReturnSummary, '11:35'),

        // exec-cards 验收段：已收束回合（执行卡四体齐）+ 在途回合（不折）
        mkEv(105, 'turn_started', 'a3', 'r3', {}, '11:40'),
        mkEv(106, 'tool_called', 'a3', 'r3', {
          tool: 'fs_patch', seq: 'r4:i0',
          input: { path: 'src/recipes.ts', old: 'export function list() {\n  return recipes\n}', new: 'export function list() {\n  return recipes.filter((r) => r.visible)\n}' },
        }, '11:40'),
        mkEv(107, 'tool_result', 'a3', 'r3', { tool: 'fs_patch', ok: true, result: { output: { patched: 'src/recipes.ts', diff_added: 3, diff_removed: 3 } } }, '11:41'),
        mkEv(108, 'tool_called', 'a3', 'r3', { tool: 'bash', input: { cmd: 'npm test -- --run src/recipes.test.ts' }, seq: 'r4:i1' }, '11:41'),
        mkEv(109, 'tool_result', 'a3', 'r3', { tool: 'bash', ok: true, result: { output: { exit_code: 0, timed_out: false, stdout: ' ✓ src/recipes.test.ts (8 tests) 24ms\n\n Test Files  1 passed\n      Tests  8 passed', stderr: '' } } }, '11:44'),
        mkEv(110, 'tool_called', 'a3', 'r3', { tool: 'fs_write', input: { path: 'src/plan.ts', bytes: 1840 }, seq: 'r4:i2' }, '11:45'),
        mkEv(111, 'tool_result', 'a3', 'r3', { tool: 'fs_write', ok: true, result: { output: { written: 'src/plan.ts' } } }, '11:45'),
        mkEv(112, 'tool_called', 'a3', 'r3', { tool: 'artifact_write', input: { path: 'docs/impl-notes.md', bytes: 920, kind: '结构说明' }, seq: 'r4:i3' }, '11:46'),
        mkEv(113, 'tool_result', 'a3', 'r3', { tool: 'artifact_write', ok: true, result: { output: { artifact_id: 'art16' } } }, '11:46'),
        mkEv(114, 'artifact_delivered', 'a3', 'r3', { path: 'docs/impl-notes.md', kind: '结构说明', version: 1, status: 'valid' }, '11:46'),
        mkMsg(115, 'agent_message', 'a3', 'r3', 'a3', '补丁打上：列表只出可见项；`recipes.test.ts` 8 过。实现要点记到 `docs/impl-notes.md`。', '11:50'),
        mkEv(116, 'turn_finished', 'a3', 'r3', {}, '11:52'),

        // 在途回合：不收摘要行；bash 卡带在途运行环（浏览器 dev 无 tool-output 推送，
        // 输出区空白属真实表现——真通道只在 Tauri 下供数）。
        mkEv(117, 'turn_started', 'a4', 'r3', {}, '11:55'),
        mkEv(118, 'tool_called', 'a4', 'r3', { tool: 'fs_read', input: { path: 'api/spec.md' }, seq: 'r5:i0' }, '11:55'),
        mkEv(119, 'tool_result', 'a4', 'r3', { tool: 'fs_read', ok: true, result: { output: { content: '# 接口说明 v2\n…' } } }, '11:55'),
        mkEv(120, 'tool_called', 'a4', 'r3', { tool: 'bash', input: { cmd: 'cargo test --workspace' }, seq: 'r5:i1' }, '11:56'),
      ]
      // gap B1/B3 截图比对（workbench#fresh）：`?mockintake` 时只留开场三条——负责人还没开口，出首屏 + 草案卡
      if (mockIntakeDraft() !== null) all.splice(3)
      // 票 08 截图比对（managed#running）：`?mockmanaged` 时追加托管开启事实与托管决定行
      if (mockManaged()) all.push(
        mkEv(121, 'system', null, null, { kind: 'delegation_changed', enabled: true, paused: false, change: 'on', approval_mode: 'assisted' }, '11:57'),
        mkEv(122, 'system', 'a4', null, { kind: 'managed_decision', card_id: 'q-m1', card_kind: 'permission', action: 'allow', reason: '端到端测试需要浏览器内核，下载落在项目缓存目录，属本项目联网委托。', via: 'model', outcome: 'allowed', actor: 'managed:g1:1' }, '11:58'),
        mkEv(123, 'system', 'a3', null, { kind: 'managed_decision', card_id: 'q-m2', card_kind: 'stamp', action: 'accept', reason: '与项目说明「单用户离线工具」一致；另两个方案偏装饰。', via: 'model', outcome: 'stamped', actor: 'managed:g1:1' }, '11:59'),
        mkEv(124, 'system', 'a5', null, { kind: 'managed_decision', card_id: 'q-m3', card_kind: 'push', action: 'deny', reason: '该目标不是项目已配置的发布目标（新增发布目标不在托管范围内）。', via: 'host', outcome: 'not_pushed', actor: 'managed:g1:1' }, '12:00'),
      )
      // 票 08 截图比对（feedback#model-recovery）：`?mockrecovery` 时追加 D11 ①②③⑥ 四种状态
      if (typeof location !== 'undefined' && new URLSearchParams(location.search).has('mockrecovery')) {
        const fail = (code: string, at: string) => ({ code, model: 'claude-opus-5-5', model_slot: 'decision', request_id: `req-${at}`, http_status: code === 'authentication' ? 401 : code === 'provider_unavailable' ? 503 : null, message: code })
        const rec = (id: string, state: string, extra: Record<string, unknown>) => ({ recovery_id: id, project_root: '/mock', purpose: 'pm_route', agent_id: 'a0', stage_run_id: null, target_role: '产品策划', state, history: [], can_retry: true, partial_text: null, unfinished: false, retry_deadline_ms: null, instruction: null, last_refusal: null, superseded_reason: null, held_instruction: null, chat_model_slot: null, ...extra })
        all.push(
          mkEv(131, 'system', 'a0', null, { kind: 'model_request_retrying', ...rec('mr1', 'retrying', { history: [fail('transport', '1')], retry_deadline_ms: Date.now() + 38000, retry_window_ms: 60000 }) }, '12:01'),
          mkEv(132, 'system', 'a0', null, { kind: 'model_request_failed', ...rec('mr2', 'stopped', { history: [fail('transport', '1'), fail('provider_unavailable', '2'), fail('provider_unavailable', '3')], chat_model_slot: 'gpt-6' }) }, '12:02'),
          mkEv(133, 'system', 'a0', null, { kind: 'model_request_failed', ...rec('mr3', 'failed', { history: [fail('authentication', '1')] }) }, '12:03'),
          mkEv(134, 'system', 'a0', null, { kind: 'model_request_recovered', ...rec('mr4', 'recovered', { history: [fail('transport', '1')] }) }, '12:04'),
        )
      }
      const after = args?.after == null ? null : Number(args.after)
      const limit = Number(args?.limit ?? 500)
      return all.filter((i) => after == null || i.event.id > after).slice(0, limit) as T
    }
    case 'data_boundary':
      return { credential_backend: 'dev_file', project_open: true, recipients: [
        { kind: 'model', endpoint: 'https://api.example.test', transport: 'http', enabled: true },
        { kind: 'mcp', endpoint: null, transport: 'stdio', enabled: true },
      ] } as T
    case 'pending_questions':
      // 票 08：`?mockmanaged` 时托管事项不进人工待决（宿主 owner_pending 同口径）
      if (mockManaged() && MOCK_MANAGED.enabled && !MOCK_MANAGED.paused) return [] as T
      return [
        { id: 'q-rec', kind: 'recovery', agent_id: 'a1', payload: { run_id: 'r9', stage: '部署演练' }, state: 'queued' },
        { id: 'q-stall', kind: 'stall', agent_id: 'a3', payload: { branch: 'no_reply', retry: true, role: '前端', instruction: '把列表过滤修一下' }, state: 'queued' },
        { id: 'q-pub', kind: 'push', agent_id: 'a0', payload: { remote: 'origin', baseline: 'main', commit_count: 12, stage_count: 3, warning: '不可逆：代码与产物将离开本机' }, state: 'queued' },
        { id: 'q-stamp', kind: 'stamp', agent_id: 'a1', payload: { run_id: 'r3', stage: '实现' }, state: 'queued' },
        { id: 'q-esc', kind: 'escalation', agent_id: 'a1', payload: { flag_id: 'f12', target: 'api/spec.md' }, state: 'queued' },
        { id: 'q-perm', kind: 'permission', agent_id: 'a0', payload: { tool: 'bash', input: { cmd: 'cargo test --workspace' }, reason: '检验前全量回归', safety_net: false }, state: 'queued' },
        { id: 'q-perm2', kind: 'permission', agent_id: 'a1', payload: { tool: 'bash', input: { cmd: 'rm -rf target && cargo build' }, reason: '构建产物损坏，安全网必问', safety_net: true }, state: 'queued' },
        { id: 'q-prop', kind: 'stamp', agent_id: 'a1', payload: { proposal_id: 'p2', surface: '工具白名单', warnings: ['改动基线权限'], warning_text: '改动基线权限' }, state: 'queued' },
        ...(mockDesign() ? [{ id: 'q-design', kind: 'escalation', agent_id: 'a2', payload: { sub: 'design_direction', revision: 1 }, state: 'queued' }] : []),
        ...mockWorktreeCard(),
        ...(mockComputer() ? [{ id: 'q-computer', kind: 'permission', agent_id: 'a3', payload: { tool: 'computer_action', safety_net: true, reason: 'safety net: desktop action with unverified external effects',
          // gap D8：点位对准示意图里的绿色开关（1300–1356 × 306–334），载荷带显示名与像素尺寸，目标圈可画
          input: { op: 'click', snapshot_id: 'snap-1', x: 1328, y: 320, element_id: 'screen-recording-toggle', reason: '验证截图权限提示' },
          app: { name: '系统设置', title: '隐私与安全性', bundle_id: 'com.apple.systempreferences' },
          screenshot: { screenshot_path: '/mock/project/.hexagon/computer-use/screenshots/mock.png', snapshot_id: 'snap-1', title: '隐私与安全性', bundle_id: 'com.apple.systempreferences', image_width: 1600, image_height: 1000 } }, state: 'queued' }] : []),
      ].filter((q) => !MOCK_DECIDED.has(q.id)) as T
    case 'usage':
      return {
        rows: [
          { agent_id: 'a0', model: 'mock-chat', stage: '需求', prompt_tokens: 18000, completion_tokens: 4200, tool_output_tokens: 0, cost_mc: 7400, calls: 8 },
          { agent_id: 'a1', model: 'mock-chat', stage: '接口', prompt_tokens: 42000, completion_tokens: 12800, tool_output_tokens: 2100, cost_mc: 19600, calls: 17 },
          { agent_id: 'a1', model: 'mock-chat', stage: '实现', prompt_tokens: 19000, completion_tokens: 5600, tool_output_tokens: 900, cost_mc: 8200, calls: 7 },
          { agent_id: 'a2', model: 'mock-chat', stage: '界面稿', prompt_tokens: 22000, completion_tokens: 9100, tool_output_tokens: 0, cost_mc: 10400, calls: 9 },
          { agent_id: 'a3', model: 'mock-code', stage: '实现', prompt_tokens: 36000, completion_tokens: 14200, tool_output_tokens: 3200, cost_mc: 17800, calls: 14 },
          { agent_id: 'a4', model: 'mock-code', stage: '实现', prompt_tokens: 28000, completion_tokens: 9800, tool_output_tokens: 2600, cost_mc: 13500, calls: 11 },
          { agent_id: 'a5', model: 'mock-chat', stage: '实现', prompt_tokens: 6400, completion_tokens: 1800, tool_output_tokens: 0, cost_mc: 2900, calls: 3 },
        ],
        total: { spent_mc: 81400, limit_cents: 20000, tokens: 286400 },
      } as T
    case 'usage_rows':
      return mockReply<UsageSummary>('usage').rows as unknown as T
    case 'usage_context_pressure':
      return { overflow_cards_14d: 1, compactions_14d: 3 } as T
    case 'usage_series': {
      // 按 bucket × agent 的 mock 序列（粒度/范围参数在 mock 里不强模拟过滤）
      const mk = (bucket: string, agent: string, p: number, c: number, t: number, cost: number) =>
        ({ bucket, agent_id: agent, prompt_tokens: p, completion_tokens: c, tool_output_tokens: t, cost_mc: cost })
      if (args?.granularity === 'hour') {
        return [
          mk('2026-09-18 09:00', 'a0', 3200, 900, 0, 1500),
          mk('2026-09-18 09:00', 'a1', 8400, 2800, 600, 4200),
          mk('2026-09-18 09:00', 'a2', 6100, 2400, 0, 3100),
          mk('2026-09-18 10:00', 'a1', 7200, 2600, 700, 3800),
          mk('2026-09-18 10:00', 'a3', 9800, 3900, 1400, 5600),
          mk('2026-09-18 10:00', 'a4', 8600, 3100, 1200, 4900),
          mk('2026-09-18 11:00', 'a4', 5400, 1900, 800, 3200),
          mk('2026-09-18 11:00', 'a5', 2800, 900, 0, 1400),
        ] as T
      }
      // 票 12：按天的 mock 改为相对今天的近 60 个 UTC 日并按 from/to 过滤——用量详情的 30 天面积图、
      // 环比与「每 Agent 最近使用」在浏览器预览里才有可看的形状（只影响 dev mock）。
      const from = (args?.from as string | null | undefined) ?? null
      const to = (args?.to as string | null | undefined) ?? null
      const weights: [string, number][] = [['a1', 1], ['a3', 0.78], ['a0', 0.48], ['a2', 0.41], ['a4', 0.24], ['a5', 0.12]]
      const out: ReturnType<typeof mk>[] = []
      for (let back = 59; back >= 0; back--) {
        const day = new Date(Date.now() - back * 864e5).toISOString().slice(0, 10)
        if ((from && day < from) || (to && day > to)) continue
        const i = 59 - back
        const wave = 0.55 + 0.45 * Math.sin(i / 4.2) ** 2 + i / 120 - (new Date(`${day}T00:00:00Z`).getUTCDay() % 6 === 0 ? 0.35 : 0)
        for (const [agent, w] of weights) {
          const p = Math.round(9000 * w * wave)
          out.push(mk(day, agent, p, Math.round(p * 0.44), Math.round(p * 0.2), Math.round(p * 0.6)))
        }
      }
      return out as T
    }
    case 'set_usage_limit':
      return null as T
    case 'autonomy':
      // 票 01：浏览器预览与新项目默认档对齐（真项目以库里的值为准）
      return 'L4' as T
    case 'owner_away':
      return null as T
    case 'owner_back':
      return {
        since_event: 0, deliveries: [], reviews: { passed: 0, rejected: 0 },
        flags: { submitted: 0, adjudicated: 0, escalated: 0 },
        permissions: { asked: 0, allowed: 0, denied: 0 },
        stages: { finished: 0, skipped: 0, rewound: 0 }, pending_todos: [],
        attention: { unresolved_actions: 0, unknown_cost_records: 0, budget_stops: 0, exception_decisions: 0, policy_candidates: 0 },
      } satisfies ReturnSummary as T
    case 'log_enabled':
      return true as T
    case 'diagnostic_records': {
      // diagnostic-records 票 01：浏览器预览同一份账本形状——过滤口径
      // 与 core records() 对齐（project=null → 只剩宿主类；选中项目 →
      // 本项目 + 宿主类；最新在前）。
      const cls = (args?.class as string | null | undefined) ?? null
      const proj = (args?.project as string | null | undefined) ?? null
      const all = [
        { ts: '2026-09-18T11:02:03Z', class: '宿主', level: 'debug', project: null, agent: null, activation: null, trace: null, branch: 'startup', code: 'app_start', ms: 0 },
        { ts: '2026-09-18T11:02:04Z', class: '宿主', level: 'debug', project: null, agent: null, activation: null, trace: null, branch: 'credentials', code: 'dev_file', ms: 0 },
        { ts: '2026-09-18T11:02:05Z', class: '宿主', level: 'debug', project: null, agent: null, activation: null, trace: null, branch: 'sandbox', code: 'seatbelt', ms: 0 },
        { ts: '2026-09-18T11:05:12Z', class: '判定', level: 'debug', project: 'p1', agent: 'a0', activation: 'sr1', trace: '512', branch: 'permission', code: 'autonomy_allow', ms: 3 },
        { ts: '2026-09-18T11:06:41Z', class: '判定', level: 'debug', project: 'p1', agent: 'a1', activation: 'sr1', trace: '560', branch: 'pm_route', code: 'dispatch:后端', ms: 812 },
        { ts: '2026-09-18T11:08:20Z', class: '拒绝', level: 'warn', project: 'p1', agent: 'a2', activation: 'sr1', trace: '590', branch: 'execute_judgment', code: 'mechanical_block', ms: 1 },
        { ts: '2026-09-18T11:09:33Z', class: '槽位', level: 'warn', project: 'p1', agent: 'a2', activation: 'sr1', trace: null, branch: 'execute_judgment', code: 'jev_unbound', ms: 0 },
        { ts: '2026-09-18T11:10:01Z', class: '槽位', level: 'debug', project: 'p1', agent: 'a0', activation: 'sr1', trace: null, branch: 'turn_dispatch', code: 'fallback_default:chat', ms: 0 },
      ]
      return all
        .filter((r) => (cls ? r.class === cls : true))
        .filter((r) => (proj ? r.project === proj || r.class === '宿主' : r.class === '宿主'))
        .reverse() as T
    }
    case 'set_agent_avatar':
      mockAvatars[String(args?.agentId)] = String(args?.dataUrl)
      return null as T
    case 'clear_agent_avatar':
      delete mockAvatars[String(args?.agentId)]
      return null as T
    case 'agent_avatar': {
      const id = String(args?.agentId)
      const role = (mock('team') as { id: string; role: string }[]).find((r) => r.id === id)?.role
      return (mockEffective(id, role) ?? null) as T
    }
    case 'role_avatar':
      return (mockRoleAvatars[String(args?.role)] ?? null) as T
    case 'set_role_avatar':
      mockRoleAvatars[String(args?.role)] = String(args?.dataUrl)
      return null as T
    case 'clear_role_avatar':
      delete mockRoleAvatars[String(args?.role)]
      return null as T
    case 'artist_proposal':
      return mockArtistProposal(String(args?.id)) as T
    case 'artist_regenerate': {
      const p = mockArtistProposal(String(args?.id))
      mockArtistProposals[p.id] = { ...p, items: p.items.map((it) => it.role === String(args?.role)
        ? { ...it, image: mockArtistImage(it.role, 1), failure: null } : it) }
      return mockArtistProposals[p.id] as T
    }
    case 'artist_apply': {
      const p = mockArtistProposal(String(args?.id))
      const roles = (args?.roles as string[] | undefined) ?? []
      for (const role of roles) {
        const img = p.items.find((it) => it.role === role)?.image
        if (img) mockRoleAvatars[role] = img
      }
      mockArtistProposals[p.id] = { ...p, applied: roles }
      return mockArtistProposals[p.id] as T
    }
    case 'artifact_content':
      return artLatest(String(args?.path)) as T
    case 'artifact_content_at':
      return (ART_CONTENT[String(args?.path)]?.[Number(args?.version)] ?? null) as T
    case 'session_summary': return (mockSessionOn() ? mockSessionSummary() : MOCK_SESSION_SUMMARY) as T
    case 'session_pause':
    case 'session_reconnect':
    case 'session_resume':
    case 'session_continue_trigger':
    case 'session_schedule_once':
    case 'session_request_worktree':
    case 'confirm_worktree':
    case 'reject_worktree':
    case 'session_verify_worktree':
      return null as T
    case 'push_targets':
      return { baseline: 'main', upstream: 'origin', remotes: ['origin'] } as T
    case 'request_push':
      return { question_id: 'q-pub', pushed: null } as T
    case 'set_agent_sleeping':
    case 'remove_agent':
    case 'review_proposal':
    case 'confirm_proposal':
    case 'reject_proposal':
    case 'rollback_proposal':
    case 'confirm_push':
    case 'reject_push':
    case 'reject_stamp':
    case 'adjudicate_flag':
    case 'allow_project_permission':
    case 'answer_permission':
    case 'stamp':
    case 'sleep_all':
    case 'send_message':
      return null as T
    // 票 16a（composer#attach 比对）：dev 下粘贴 / 拖入 / + 选图能出缩略图——原先 mock 落到 null，
    // Composer 读 media_type 抛错只弹红 toast，附件区截不到。
    case 'stage_attachment': {
      const name = String(args?.name ?? 'image.png')
      const ext = name.split('.').pop()?.toLowerCase() ?? 'png'
      const media = ({ jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp' } as Record<string, string>)[ext] ?? 'image/png'
      return { media_type: media, path: `.hexagon/inbox/mock-${Date.now()}.${ext}`, bytes: (args?.bytes as unknown[] | undefined)?.length ?? 0, name } as T
    }
    case 'discard_attachments': return null as T
    case 'project_skill_document': return { project_root: '/mock', skill: String(args?.skill), digest: 'mock', content: '# Project skill' } as T
    case 'save_project_skill_document': return args?.document as T
    case 'experience_limits': return { entry_chars: 2000, load_count: 10, load_chars: 8000 } as T
    case 'set_experience_limits': return args?.limits as T
    case 'recover_experience': return [] as T
    case 'experience_history': return { items: [], next_cursor: null } as T
    // 功能缺口 D3：预览里项目技能 repo-notes 带两条经验（原型 settings#skills 的两行：有效 / 内容变更需复审），其余技能为空。
    case 'experience_entries': {
      if (args?.skill !== 'repo-notes') return [] as T
      const entry = (entry_id: string, body: string, revision: number, author: string) => ({ schema_version: 1, entry_id, revision, project_id: 'mock', skill: 'repo-notes', body, notes: '',
        conditions: { roles: [], stages: [], paths: [] }, sources: [{ review_event: 12, activation: 3, artifact_id: 'review-12', author, artifact_version: revision, artifact_digest: 'mock' }] })
      return [
        { entry: entry('safari-private', 'Safari 隐私模式下 setItem 会抛错', 3, '前端技术负责人复审'), state: 'active', match_reason: 'matched', recovery_pending: false, source_count: 1 },
        { entry: entry('cache-hash', '缓存名需带构建哈希', 1, 'QA 测试记录'), state: 'changed', match_reason: 'matched', recovery_pending: false, source_count: 1 },
      ] as T
    }
    case 'experience_proposal': return null as T
    case 'proposals':
      return [
        { id: 'p2', artifact_path: 'proposals/p2.md', surface: 'pack_copy', target: 'grants', status: 'awaiting_stamp', author: 'a4' },
        // 票 08：in_review 待负责人裁决（复审 agent 无工具面）
        { id: 'p3', artifact_path: 'proposals/p3.md', surface: 'agents_md', target: 'AGENTS.md', status: 'in_review', author: 'a2' },
      ] as T
    case 'mcp_services':
      return [] as T
    // owner-followups-1015 #14：设置·发布页预览——两条目标，npm 的令牌已存、OTP 未设置（只回名字与「已存入」）。
    case 'release_targets': return MOCK_RELEASE.map((r) => ({ ...r, credentials: r.credentials.map((c) => ({ ...c })) })) as T
    case 'save_release_target': return mockSaveRelease(args?.draft as ReleaseTargetDraft) as T
    case 'remove_release_target': MOCK_RELEASE = MOCK_RELEASE.filter((r) => r.id !== args?.id); return null as T
    case 'set_release_credential':
    case 'remove_release_credential': {
      const target = MOCK_RELEASE.find((r) => r.id === args?.id)
      const cred = target?.credentials.find((c) => c.name === args?.credential)
      if (cred) cred.set = cmd === 'set_release_credential'
      return target as T
    }
    case 'request_release': return 'q-rel' as T
    case 'confirm_release': {
      const target = MOCK_RELEASE.find((r) => r.id === 'rel-npm')
      return { target_id: target?.id ?? '', name: target?.name ?? '', output: '', duration_ms: 0 } as T
    }
    case 'reject_release': return null as T
    case 'approve_release_target_card': return (MOCK_RELEASE[0] ?? null) as T
    case 'reject_release_target_card': return null as T
    case 'list_mcp_entries':
      return [
        { name: 'termius', command: 'ssh-mcp', args: ['--stdio'], env: {}, cwd: null, disabled: false, transport: 'stdio', url: null, origin: 'global' },
        { name: 'web-svc', command: '', args: [], env: {}, cwd: null, disabled: true, transport: 'remote', url: 'https://h/sse', origin: 'global' },
      ] as T
    case 'save_mcp_service':
    case 'delete_mcp_service':
    case 'open_browser_extension_store':
    case 'open_mcp_market':
      return null as T
    case 'scan_external_mcp':
      return [
        { reference: 'mock-figma', name: 'figma', command: 'npx', args: ['-y', 'figma-mcp'], env: {}, cwd: null, disabled: false, transport: 'stdio', url: null, origin: 'cursor', source_path: '~/.cursor/mcp.json', conflict: false },
        { reference: 'mock-web', name: 'web-svc', command: '', args: [], env: {}, cwd: null, disabled: false, transport: 'remote', url: 'https://h/sse', origin: 'claude', source_path: '~/.claude.json', conflict: true },
      ] as T
    case 'import_mcp':
      return { imported: 1, skipped: ['web-svc: conflict'] } as T
    case 'repo_paths':
      return ['src/', 'src/api.rs', 'docs/', 'AGENTS.md'] as T
    case 'list_repo_dir': {
      const rel = String(args?.rel ?? '')
      if (rel === '' || rel === '.') {
        return [
          { name: '.hexagon', path: '.hexagon', kind: 'dir' },
          { name: 'src', path: 'src', kind: 'dir' },
          { name: 'README.md', path: 'README.md', kind: 'file' },
          { name: 'package.json', path: 'package.json', kind: 'file' },
        ] as T
      }
      if (rel === 'src') {
        return [{ name: 'main.ts', path: 'src/main.ts', kind: 'file' }] as T
      }
      if (rel === '.hexagon') {
        return [{ name: 'specs', path: '.hexagon/specs', kind: 'dir' }] as T
      }
      if (rel === '.hexagon/specs') {
        return [{ name: 'prd.md', path: '.hexagon/specs/prd.md', kind: 'file' }] as T
      }
      return [] as T
    }
    case 'read_repo_file': {
      const p = String(args?.path)
      // .hexagon/ 下的产物磁盘版与 ART_CONTENT 最新版对齐，
      // 浏览器预览里「文件页 vs 版本基线」才有真实差异可看。
      const artPath = p.startsWith('.hexagon/') ? p.slice('.hexagon/'.length) : null
      const art = artPath ? artLatest(artPath) : null
      if (art != null) return art as T
      if (p.endsWith('.md')) {
        return `# ${p}\n\n- 列表项一\n- 列表项二\n\n正文 **加粗** 与 \`code\`。\n` as T
      }
      return `// ${p}\n` as T
    }
    case 'read_repo_file_snapshot':
      return { path: String(args?.path), content: mock<string>('read_repo_file', args),
        project_root: String(args?.expectedProjectRoot), generation: useUiStore.getState().projectEpoch } as T
    case 'repo_file_head':
      // mock 仓假定每个文本文件都有 HEAD 版：比盘上少一行，diff 可见。
      return `# HEAD 版 ${args?.path}\n` as T
    case 'write_repo_file':
    case 'create_repo_file':
    case 'create_repo_dir':
    case 'reveal_repo_path':
    case 'open_with':
      return null as T
    // #31：mock 机器上装了两个编辑器，图标取不到（中性图标）。
    case 'open_with_candidates':
      return [{ id: 'com.microsoft.VSCode', name: 'Visual Studio Code', icon: null }, { id: 'dev.zed.Zed', name: 'Zed', icon: null }] as T
    case 'permission_shape_suggestion':
      return (args?.questionId === 'q-perm' ? { shape: 'exact:cargo test --workspace', generalized: false, tool: 'bash', project_id: 'p1', agent_id: 'a0' } : null) as T
    case 'approval_mode':
      return { project_id: 'p1', project_root: '/mock', mode: 'restricted' } as T
    case 'set_approval_mode':
      return { project_id: 'p1', project_root: '/mock', mode: args?.mode } as T
    case 'project_delegation':
      if (mockManaged()) return { ...MOCK_MANAGED, generation: 1, delegation_id: 'mock-grant' } as T
      return { enabled: false, paused: false, generation: 0, delegation_id: null } as T
    case 'set_project_delegation':
      if (mockManaged()) MOCK_MANAGED = { enabled: Boolean(args?.enabled), paused: false }
      return { enabled: Boolean(args?.enabled), paused: false, generation: 1, delegation_id: args?.enabled ? 'mock-grant' : null } as T
    case 'set_project_delegation_paused':
      if (mockManaged()) MOCK_MANAGED = { enabled: true, paused: Boolean(args?.paused) }
      return { enabled: true, paused: Boolean(args?.paused), generation: 2, delegation_id: 'mock-grant' } as T
    case 'stop_model_request':
      return null as T
    case 'managed_history':
      return (mockManaged() ? MOCK_MANAGED_HISTORY : []) as T
    case 'managed_external_ops':
      return (mockManaged() ? MOCK_MANAGED_EXTERNAL : []) as T
    case 'managed_summary': {
      const at = (msAgo: number) => new Date(Date.now() - msAgo).toISOString().slice(0, 19).replace('T', ' ')
      return (mockManaged()
        ? { started_at: at(42 * 60_000), ended_at: MOCK_MANAGED.enabled ? null : at(0), limit_cents: 10_000, spent_mc: 4_540_000, session_spent_mc: 680_000, cost_unknown: false }
        : { started_at: null, ended_at: null, limit_cents: null, spent_mc: 0, session_spent_mc: 0, cost_unknown: false }) as T
    }
    case 'export_managed_report':
      return null as T
    case 'managed_processing':
      return (mockManaged() && MOCK_MANAGED.enabled ? [
        { id: 'q-m4', kind: 'stamp', agent_id: 'a3', payload: { stage: '测试' }, state: 'queued' },
        { id: 'q-m1', kind: 'permission', agent_id: 'a4', payload: {}, state: 'queued' },
      ] : []) as T
    case 'design_direction':
      // 票 04 截图比对（decisions#design）：`?mockdesign` 时给一张 D12 视觉方向卡的三套方案；默认关，既有测试依赖空夹具
      if (mockDesign()) return { project_id: 'p1', revision: 1, question_id: 'q-design', state: 'pending', selected_option: null, existing_guidance: null, selected_at: null, options: MOCK_DESIGN_OPTIONS,
        previous_guidance: { source: 'selected', title: '清爽工具', summary: 'Inter · 蓝灰 · 8px 圆角', recorded_at: '2026-10-01 09:00:00' } } as T
      return { project_id: 'p1', revision: 0, question_id: null, state: 'unconfirmed', options: [], selected_option: null, existing_guidance: null, selected_at: null, previous_guidance: null } as T
    case 'choose_design_direction':
    case 'desktop_screenshot':
      if (cmd === 'desktop_screenshot' && mockComputer()) return { name: 'mock.png', data_url: MOCK_COMPUTER_SHOT } as T
      throw new Error('Desktop app required')
    case 'browser_selection_poll': return [] as T
    case 'browser_selection_start':
    case 'browser_selection_discard': return undefined as T
    case 'computer_models':
      return { project_root: '/mock', roles: [], vision_slots: [] } as T
    case 'desktop_status':
      return { project_root: '/mock', enabled: false, active_project: null, active_agent: null, busy: false, paused: false, outcome_unknown: false, screenshot_count: 0 } as T
    case 'desktop_control':
    case 'desktop_leave_project':
      throw new Error('Desktop app required')
    case 'desktop_permissions':
      return { host_name: 'Hexagon', supported: false, available: false, accessibility: false, screen_recording: false, input_events: false, ready: false, error: 'Desktop app required' } as T
    case 'desktop_open_settings':
      throw new Error('Desktop app required')
    case 'sandbox_status':
      return { mode: 'seatbelt', available: true, note: 'mock sandboxed' } as T
    case 'list_skills':
      return [
        { name: 'spec-writing', description: '规格书写规范', origin: 'builtin', enabled: true },
        { name: 'code-review', description: '评审清单', origin: 'builtin', enabled: true },
        { name: 'my-lint', description: '自建检查器', origin: 'global', enabled: false },
        { name: 'repo-notes', description: '本项目笔记规范', origin: 'project', enabled: true },
      ] as T
    case 'skill_files':
      return ['SKILL.md', 'scripts/check.sh'] as T
    case 'read_skill_file':
      return `---\nname: ${args?.name}\ndescription: mock 技能\n---\n\n## 用法\n\n按需加载。` as T
    case 'save_global_skill':
      return null as T
    case 'scan_external_skills':
      return [
        { name: 'skill-creator', description: '造技能的技能', origin: 'claude', path: '~/.claude/skills/skill-creator', conflict: false },
        { name: 'banner-design', description: '横幅设计', origin: 'claude', path: '~/.claude/skills/banner-design', conflict: true },
        { name: 'dev-guide', description: '开发规范', origin: 'cursor', path: '~/.cursor/skills/dev-guide', conflict: false },
      ] as T
    case 'import_skills':
      return { imported: 1, skipped: ['dup: conflict'] } as T
    case 'install_skill_path':
      return 'installed-skill' as T
    case 'reviewer_mode':
      return 'shadow' as T
    // ---- 项目向导 mock：浏览器 dev 始终「已有项目」，向导只在 Tauri 真开时出现 ----
    case 'project_open':
      // ui-redesign 票 10：`?launcher` 让浏览器 dev 落到启动页，用来和原型 entry#launcher 截图比对
      return !new URLSearchParams(location.search).has('launcher') as T
    case 'project_identity':
      return (currentProjectIdentity() ?? { project_root: useUiStore.getState().projectRoot ?? '/tmp/hexagon-demo', generation: 1 }) as T
    case 'recent_projects':
      // 一条存在 + 一条已删目录（exists=false）——浏览器 dev 可目检红标与移除钮
      return [
        { dir: '/tmp/hexagon-demo', name: 'hexagon-demo', mode: 'pack', opened_at: Math.floor(Date.now() / 1000) - 7200, exists: true },
        { dir: '/tmp/landing-page', name: 'landing-page', mode: 'fastpath', opened_at: Math.floor(Date.now() / 1000) - 86400, exists: true },
        { dir: '/gone/deleted-proj', name: 'deleted-proj', mode: 'fastpath', opened_at: 0, exists: false },
      ] as T
    case 'open_recent':
    case 'remove_recent':
    case 'close_project':
      return null as T
    case 'project_info':
      return { name: '食谱 App', mode: 'pack', pack_name: '规格驱动', fastpath_agent_id: null, fastpath_role: null } as T
    case 'dispatch':
    case 'upgrade_to_pack':
      return null as T
    case 'read_instruction_file':
      return '已有项目说明' as T
    case 'draft_role_duty':
      return '起草的职责' as T
    case 'draft_flow':
      return {
        name: '生成的流程', version: 1,
        stages: [{
          name: '实现', roles: ['后端'], due: ['代码'], checks: ['npm test'],
          reviews: [], stamp_point: true, backfill_edges: [['后端', 'QA']], consult_wake: ['架构师'],
        }],
        knobs: { judge: null, flag_patience: null, auto_backfill: null, consult_auto_wake: null },
      } as T
    case 'inspect_dir':
      // 目录名含 empty 时按空目录体检：浏览器 dev 可走到项目说明步的答问卡（票 10 截图比对）
      if (String(args?.dir ?? '').includes('empty')) {
        return { exists: true, empty: true, is_git: true, dirty: false, has_workbench: false, instructions: null } as T
      }
      return { exists: true, empty: false, is_git: true, dirty: false, has_workbench: false, instructions: 'AGENTS.md' } as T
    case 'preset_roles':
      return [
        { name: '产品策划', duty: '需求与规格', reviewer: null, model_slot: 'chat', globs: [], skills: ['spec-writing'] },
        { name: '项目经理', duty: '决定下一手派给谁，或先不派活', reviewer: null, model_slot: 'chat', globs: [], skills: ['handoff-note'] },
        { name: '后端', duty: '服务端实现', reviewer: '后端技术负责人', model_slot: 'chat', globs: [], skills: [] },
      ] as T
    case 'list_role_templates':
      return [
        { def: { name: '产品策划', duty: '需求与规格', reviewer: null, model_slot: 'chat', globs: [], skills: ['spec-writing'] }, origin: 'builtin' },
        { def: { name: '项目经理', duty: '决定下一手派给谁，或先不派活', reviewer: null, model_slot: 'chat', globs: [], skills: ['handoff-note'] }, origin: 'builtin' },
        { def: { name: '后端', duty: '服务端实现', reviewer: '后端技术负责人', model_slot: 'chat', globs: [], skills: [] }, origin: 'builtin' },
        { def: { name: '插画师', duty: '出图素材', reviewer: null, model_slot: 'chat', globs: [], skills: [] }, origin: 'custom' },
      ] as T
    case 'save_role_template':
    case 'delete_role_template':
      return null as T
    case 'preset_packs':
      return [
        { name: '规格驱动', version: 1, stages: [{ name: '规格', roles: ['产品策划'], due: ['规格'], stamp_point: true }] },
      ] as T
    case 'export_events':
      return 42 as T // mock：导出条数
    case 'resolve_install':
      return { installed: args?.allow === true } as T
    case 'stall_retry':
    case 'stall_ack':
      return null as T
    case 'agent_detail':
      return {
        agent_id: 'a0', role: '后端', status: 'active', model_slot: 'chat', custom: false,
        def: { duty: '服务端实现', reviewer: '后端技术负责人', model_slot: 'chat', skills: [] },
        globs: ['src/**'], grants: [{ kind: 'skill', name: 'spec-writing' }],
      } as T
    case 'quality_configuration':
      return { version: 'mock-quality:1', project_root: '/mock-project', stages: [{ seq: 0, name: '规格', commands: {} }] } as T
    case 'update_quality_commands':
      throw new Error(i18n.t('quality.desktopOnly'))
    case 'current_process_pack':
    case 'pack_draft':
      return { name: '规格驱动', version: 1, stages: [{ name: '规格', roles: ['产品策划'], due: ['规格'], stamp_point: true }] } as T
    case 'pack_templates':
      return ['规格驱动'] as T
    case 'pack_template':
      return { name: '规格驱动', version: 1, stages: [{ name: '规格', roles: ['产品策划'], due: ['规格'], stamp_point: true }] } as T
    case 'draft_role_def':
      return '负责服务端实现与代码质量，向技术负责人汇报。' as T
    case 'update_agent':
    case 'create_role':
    case 'set_agent_grants':
      return null as T
    case 'request_grant':
      return { granted: false, question_id: 'q-grant', via: 'queued' } as T
    case 'confirm_grant':
      return { granted: args?.allow === true, question_id: args?.qid, via: 'owner' } as T
    case 'save_pack_draft':
    case 'export_pack_yaml':
      return null as T
    case 'save_pack_template':
      return '/home/user/.config/hexagon/templates/规格驱动.json' as T
    case 'create_project':
      return null as T
    case 'list_providers':
      // gap B8：`?mocknoprovider` 给 dev 一份空供应商表，看向导服务商步三字段表单（对照 entry#wz-provider）
      if (new URLSearchParams(location.search).has('mocknoprovider')) return { providers: [], slots: {} } as T
      return {
        providers: [
          {
            id: 'openrouter', name: 'OpenRouter', kind: 'openai',
            base_url: 'https://openrouter.ai/api/v1', enabled: true, key_set: true,
            models: [
              // gap B11：带展示名，dev 下消息头显示「DeepSeek V3」而不是记录的 id
              { id: 'deepseek/deepseek-chat', name: 'DeepSeek V3', caps: ['tools'] },
              { id: 'google/gemini-2.5-flash', caps: ['vision', 'tools'] },
              { id: 'deepseek/deepseek-r1:free', caps: ['free', 'reasoning', 'tools'] },
            ],
          },
          {
            id: 'anthropic', name: 'Anthropic', kind: 'anthropic',
            base_url: 'https://api.anthropic.com', enabled: false, key_set: false, models: [],
          },
        ],
        slots: {
          chat: { provider_id: 'openrouter', model: 'deepseek/deepseek-chat' },
          // `?launcher`（票 10 截图比对）下 default 槽就绪，向导能走过模型服务商步
          ...(new URLSearchParams(location.search).has('launcher') ? { default: { provider_id: 'openrouter', model: 'deepseek/deepseek-chat' } } : {}),
        },
      } as T
    case 'save_provider':
    case 'delete_provider':
    case 'set_slot_binding':
    case 'remove_slot_binding':
      return null as T
    // gap B4：dev 下钥匙含 bad 时模拟服务商拒绝（看失败原因），否则检测通过
    case 'probe_provider_key':
      return (String(args?.secret ?? '').includes('bad')
        ? { ok: false, failure: 'authentication', status: 401, model_listed: null, window: null }
        : { ok: true, failure: null, status: null, model_listed: true, window: 200000 }) as T
    // 票 21：dev 夹具只粗略模仿 core provider_url（真规则与测试在 core）。
    case 'provider_request_url': {
      const base = String(args?.baseUrl ?? '').trim().replace(/\/+$/, '')
      if (!base || base.includes('[stored:')) return null as T
      const tail = args?.kind === 'anthropic' ? '/messages' : '/chat/completions'
      if (base.endsWith(tail)) return base as T
      const versioned = /\/v\d+[a-z0-9]*(\/|$)/i.test(base.replace(/^[a-z]+:\/\/[^/]*/i, ''))
      return (args?.kind === 'jev' ? `${base}/v1/systemone` : versioned ? `${base}${tail}` : `${base}/v1${tail}`) as T
    }
    case 'provider_presets':
      return [
        mockPreset({ id: 'anthropic', name: 'Anthropic', kind: 'anthropic', base_url: 'https://api.anthropic.com', default_model: 'claude-opus-5-5', icon_key: 'anthropic' }),
        mockPreset({ id: 'openai', name: 'OpenAI', kind: 'openai', base_url: 'https://api.openai.com/v1', default_model: '', icon_key: 'openai' }),
        mockPreset({ id: 'deepseek', name: 'DeepSeek', kind: 'openai', base_url: 'https://api.deepseek.com', default_model: '', icon_key: 'deepseek' }),
        mockPreset({ id: 'moonshot', name: 'Moonshot', kind: 'openai', base_url: 'https://api.moonshot.cn/v1', default_model: '', icon_key: 'moonshot' }),
        mockPreset({ id: 'openrouter', name: 'OpenRouter', kind: 'openai', base_url: 'https://openrouter.ai/api/v1', default_model: '', icon_key: 'openrouter' }),
        // gap B13：设置页目录并进同一份预设（core provider_presets.rs）
        mockPreset({ id: 'gemini', name: 'Gemini', kind: 'openai', base_url: 'https://generativelanguage.googleapis.com/v1beta/openai', default_model: '', icon_key: 'google' }),
        mockPreset({ id: 'xai', name: 'xAI', kind: 'openai', base_url: 'https://api.x.ai/v1', default_model: '', icon_key: 'grok' }),
        mockPreset({ id: 'groq', name: 'Groq', kind: 'openai', base_url: 'https://api.groq.com/openai/v1', default_model: '', icon_key: 'groq' }),
        mockPreset({ id: 'mistral', name: 'Mistral', kind: 'openai', base_url: 'https://api.mistral.ai/v1', default_model: '', icon_key: 'mistral' }),
        mockPreset({ id: 'siliconflow', name: 'SiliconFlow', kind: 'openai', base_url: 'https://api.siliconflow.cn/v1', default_model: '', icon_key: 'silicon' }),
        mockPreset({ id: 'zhipu', name: 'Zhipu', kind: 'openai', base_url: 'https://open.bigmodel.cn/api/paas/v4', default_model: '', icon_key: 'zhipu' }),
        mockPreset({ id: 'ollama', name: 'Ollama', kind: 'openai', base_url: 'http://localhost:11434/v1', default_model: '', icon_key: 'ollama' }),
        mockPreset({ id: 'lm-studio', name: 'LM Studio', kind: 'openai', base_url: 'http://localhost:1234/v1', default_model: '', icon_key: 'lmstudio' }),
        mockPreset({ id: 'typesafe', name: 'TypeSafe', kind: 'jev', base_url: 'https://api.typesafe.ai', default_model: 'jev-latest' }),
        // #22：目录条目（core provider_catalog；节选三条：无版本段完整端点 / 有版本段 / 无公共地址）
        mockPreset({ id: 'perplexity', name: 'Perplexity', kind: 'openai', base_url: 'https://api.perplexity.ai/chat/completions', origin: 'catalog', icon_key: 'perplexity', website: 'https://perplexity.ai/', docs: 'https://docs.perplexity.ai/home' }),
        mockPreset({ id: 'dashscope', name: 'Bailian', kind: 'openai', base_url: 'https://dashscope.aliyuncs.com/compatible-mode/v1', origin: 'catalog', icon_key: 'bailian', website: 'https://www.aliyun.com/product/bailian', docs: 'https://help.aliyun.com/zh/model-studio/getting-started/', sample_models: ['deepseek-r1', 'deepseek-v3', 'deepseek-v3.1'] }),
        mockPreset({ id: 'gpustack', name: 'GPUStack', kind: 'openai', base_url: '', origin: 'catalog', icon_key: 'gpustack', website: 'https://gpustack.ai/', docs: 'https://docs.gpustack.ai/latest/' }),
      ] as T
    case 'fetch_provider_models':
      return [
        { id: 'deepseek/deepseek-chat', group: 'deepseek', caps: ['tools'] },
        { id: 'google/gemini-2.5-flash', group: 'google', caps: ['vision', 'tools'] },
        { id: 'qwen/qwen-2.5-72b', group: 'qwen', caps: ['tools', 'free'] },
      ] as T
    case 'agents_md_draft':
      return `# ${args?.name ?? 'project'}\n\n## Commands\n` as T
    case 'optimize_agents_md':
      return `# ${args?.name ?? 'project'}\n\n## 做什么\n${args?.sentence ?? ''}\n\n## Commands\n- Build:\n- Test:\n- Check:\n\n## Layout\n- 未知\n\n## Conventions\n-\n` as T
    case 'brief_questions':
      return [
        { question: '打算用什么技术栈？', options: ['React + Node', 'Python', '暂不指定'] },
        { question: '主要给谁用？', options: ['自己/家人', '对外用户'] },
      ] as T
    case 'draft_role_defs':
      return ((args?.roles as { name: string }[] | undefined) ?? []).map((r) => ({
        name: r.name,
        duty: `${r.name}在本项目中的职责`,
        globs: [],
      })) as T
    case 'run_opening_intake':
      return null as T
    // gap B1：`?mockintake` 给 dev 一份未落盘草案（对照 workbench#fresh）
    case 'confirm_intake_brief':
      mockIntake.draft = null
      mockIntake.discarded = null
      return null as T
    case 'discard_intake_draft': {
      const current = mockIntakeDraft()
      if (current !== null) mockIntake.discarded = current
      mockIntake.draft = null
      return null as T
    }
    case 'restore_intake_draft':
      if (mockIntakeDraft() !== null || !mockIntake.discarded) throw { code: 'no_intake_draft', message: 'no intake draft' }
      mockIntake.draft = mockIntake.discarded
      mockIntake.discarded = null
      return null as T
    case 'update_intake_draft':
      mockIntake.draft = String(args?.body ?? '')
      return null as T
    case 'intake_draft_pending':
      return (mockIntakeDraft() !== null) as T
    case 'intake_draft':
      return mockIntakeDraft() as T
    // ui-redesign 票 11：设置·权限「已记住的规则」表的 dev 样例（对照 settings#perms）
    case 'permission_rules':
      return [
        { id: 'r1', project_shared: false, agent_id: 'a5', tool: 'bash', shape: 'npx playwright install *', domain: null, effect: 'allow', scope: 'project', created_at: '2026-10-10 14:22', network_allowed: true, background_allowed: false, session_name: null },
        { id: 'r2', project_shared: false, agent_id: 'a2', tool: 'bash', shape: 'npm test *', domain: null, effect: 'allow', scope: 'project', created_at: '2026-10-09 18:03', network_allowed: false, background_allowed: false, session_name: null },
        { id: 'r3', project_shared: true, agent_id: null, tool: 'fs_write', shape: 'src/db/*.ts', domain: null, effect: 'allow', scope: 'project', created_at: '2026-10-08 11:40', network_allowed: false, background_allowed: false, session_name: null },
        { id: 'r4', project_shared: false, agent_id: 'a3', tool: 'web_fetch', shape: 'developer.mozilla.org', domain: 'developer.mozilla.org', effect: 'allow', scope: 'project', created_at: '2026-10-07 09:12', network_allowed: true, background_allowed: false, session_name: null },
      ] as T
    default:
      return null as T
  }
}
