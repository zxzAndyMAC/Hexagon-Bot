// 核 API 接缝：Tauri 环境走 invoke；浏览器开发环境用内置 mock 数据。
// UI 的唯一通道 = 这些命令 + 事件推送，没有旁路。类型对齐 api.rs 的 JSON 形状。

import { Channel, invoke } from '@tauri-apps/api/core'
import { CREATE_STEPS, type CreateStep } from './createProgress'
import i18n from './i18n'

// ---- IPC DTO（ADR 0054，票 06）：ui/src/gen/* 由 ts-rs 从 Rust DTO 生成 ----
// 手改禁地——字段要改改 Rust 侧，`cargo test` 重出声明，字段漂移由 tsc 抓。
// 词表字段（status/state/kind/mode）在 Rust 侧按 schema CHECK 钉了字面量联合。
import type { CmdError } from './gen/CmdError'
import type { AttachRef } from './gen/AttachRef'
import type { SandboxStatus } from './gen/SandboxStatus'
import type { QueuedCard } from './gen/QueuedCard'
import type { TurnDelta } from './gen/TurnDelta'
import type { StageRow } from './gen/StageRow'
import type { TimelineItem } from './gen/TimelineItem'
import type { TeamRow } from './gen/TeamRow'
import type { ArtifactRow } from './gen/ArtifactRow'
import type { UsageTotal } from './gen/UsageTotal'
import type { UsageRow } from './gen/UsageRow'
import type { UsageSummary } from './gen/UsageSummary'
import type { UsageBucket } from './gen/UsageBucket'
import type { StageAction } from './gen/StageAction'
import type { OpenStageOutcome } from './gen/OpenStageOutcome'
import type { CheckResult } from './gen/CheckResult'
import type { CheckOutcome } from './gen/CheckOutcome'
import type { OverrideOutcome } from './gen/OverrideOutcome'
import type { InstallOutcome } from './gen/InstallOutcome'
import type { GrantOutcome } from './gen/GrantOutcome'
import type { PublishOutcome } from './gen/PublishOutcome'
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
import type { ProviderView } from './gen/ProviderView'
import type { ProvidersView } from './gen/ProvidersView'
import type { SlotBinding } from './gen/SlotBinding'
import type { CreateProjectOpts } from './gen/CreateProjectOpts'
import type { CreateStep as CreateStepDto } from './gen/CreateStep'
import type { Event } from './gen/Event'
import type { EventKind } from './gen/EventKind'
import type { MessageRow } from './gen/MessageRow'
import type { MessageToken } from './gen/MessageToken'
import type { ExportFilter } from './gen/ExportFilter'
import type { RepoEntry } from './gen/RepoEntry'

/** 旧名薄壳——新代码直接用 QueuedCard。 */
export type PendingQuestion = QueuedCard
/** 旧名薄壳——新代码直接用 StageDef。 */
export type PackStage = StageDef

export type {
  CmdError, TurnDelta, StageRow, TimelineItem, QueuedCard, TeamRow, ArtifactRow,
  UsageTotal, UsageRow, UsageSummary, UsageBucket, StageAction, OpenStageOutcome,
  CheckResult, CheckOutcome, OverrideOutcome, InstallOutcome, PublishOutcome,
  FlagOutcome, AdjudicateOutcome, ReturnSummary, ProposalRow, ProjectInfo,
  RecentProject, TurnOutcome, DirReport, RoleDef, PackDef, StageDef, AgentPatch,
  PermissionRuleRow, SkillRow, McpServiceRow,
  AgentDetail, ModelEntry, ProviderDef, ProviderView, ProvidersView, SlotBinding,
  CreateProjectOpts, CreateStepDto, Event, EventKind, MessageRow, MessageToken, ExportFilter,
  RoleTemplate, ExtSkillRow, ImportReport, McpEntryRow, McpSpec, ExtMcpRow,
  RepoEntry,
}


export const isTauri = '__TAURI_INTERNALS__' in window || '__TAURI__' in window

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return invoke<T>(cmd, args)
  // mock 只可达于 dev 构建：import.meta.env.DEV 在产物里是字面量 false，
  // 此分支为死码被 tree-shake，~560 行夹具不进生产包（arch-review 附录 B4
  // 核验：原先走运行时 isTauri 判，摇不掉）。组件侧的 isTauri 是 UI 显隐
  // 判定，与本门控职责不同，保留运行时检查。
  if (import.meta.env.DEV) return mock<T>(cmd, args)
  throw new Error('non-Tauri production build has no backend')
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
  const { code, message } = asCmdError(e)
  const key = `errors.${code}`
  return i18n.exists(key) ? i18n.t(key) : message
}

// ---- 回合流式 delta（turn-streaming 票 03）----
// 瞬时增量通道：payload 不落库；done=true 是流终信号（成败都发），
// reset=true 表示瞬时重试、本轮已收文本作废重起。

/// 订阅回合 delta；浏览器 dev 无推送通道，返回 no-op 退订。
export async function onTurnDelta(cb: (d: TurnDelta) => void): Promise<() => void> {
  if (!isTauri) return () => {}
  const { listen } = await import('@tauri-apps/api/event')
  return listen<TurnDelta>('turn-delta', (e) => cb(e.payload))
}

export const api = {
  ping: () => call<string>('core_ping'),
  openProject: (dir: string, name: string, roles: [string, string][], packJson?: string) =>
    call<void>('open_project', { dir, name, roles, packJson: packJson ?? null }),
  timeline: (after?: number, limit = 500) =>
    call<TimelineItem[]>('timeline', { after: after ?? null, limit }),
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
  writeRepoFile: (path: string, content: string) => call<void>('write_repo_file', { path, content }),
  createRepoFile: (path: string) => call<void>('create_repo_file', { path }),
  createRepoDir: (path: string) => call<void>('create_repo_dir', { path }),
  sandboxStatus: () => call<SandboxStatus>('sandbox_status'),
  answerPermission: (questionId: string, allow: boolean, rememberShape?: string, scope = 'activation') =>
    call<void>('answer_permission', { questionId, allow, rememberShape: rememberShape ?? null, scope }),
  advance: () => call<StageAction>('advance'),
  openStage: (seq: number) => call<OpenStageOutcome>('open_stage', { seq }),
  runChecks: () => call<CheckOutcome>('run_checks'),
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
  team: () => call<TeamRow[]>('team'),
  stageStatus: () => call<StageRow[]>('stage_status'),
  pendingQuestions: () => call<PendingQuestion[]>('pending_questions'),
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
  // ui-audit-2 票 06：MCP 服务实况（.hexagon/mcp.json 配置 → 宿主状态）
  mcpServices: () => call<McpServiceRow[]>('mcp_services'),
  // 票 05：MCP 全局清单（无项目可用；配置清单≠授权）
  mcpEntries: () => call<McpEntryRow[]>('list_mcp_entries'),
  saveMcpService: (spec: McpSpec) => call<void>('save_mcp_service', { spec }),
  deleteMcpService: (name: string) => call<void>('delete_mcp_service', { name }),
  // 票 06：本机 MCP 扫描导入 + 市场
  scanExternalMcp: () => call<ExtMcpRow[]>('scan_external_mcp'),
  importMcp: (specs: McpSpec[]) => call<ImportReport>('import_mcp', { specs }),
  openMcpMarket: () => call<void>('open_mcp_market'),
  usage: () => call<UsageSummary>('usage'),
  usageSeries: (granularity: 'day' | 'hour' = 'day', from?: string | null, to?: string | null) =>
    call<UsageBucket[]>('usage_series', { granularity, from: from ?? null, to: to ?? null }),
  setUsageLimit: (limitCents: number | null) =>
    call<void>('set_usage_limit', { limitCents }),
  setLogEnabled: (enabled: boolean) => call<void>('set_log_enabled', { enabled }),
  logEnabled: () => call<boolean>('log_enabled'),
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
  requestPublish: (remote: string) => call<string>('request_publish', { remote }),
  confirmPublish: (qid: string) => call<PublishOutcome>('confirm_publish', { qid }),
  rejectPublish: (qid: string) => call<void>('reject_publish', { qid }),
  // ---- 崩溃恢复（票 37）----
  recoverRun: (runId: string) => call<void>('recover_run', { runId }),
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
  // ---- 头像 ----
  setAgentAvatar: (agentId: string, dataUrl: string) =>
    call<void>('set_agent_avatar', { agentId, dataUrl }),
  agentAvatar: (agentId: string) => call<string | null>('agent_avatar', { agentId }),
  // ---- 启动页 / 最近项目（票 29）----
  recentProjects: () => call<RecentProject[]>('recent_projects'),
  openRecent: (dir: string) => call<void>('open_recent', { dir }),
  closeProject: () => call<void>('close_project'),
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
  agentsMdDraft: (name: string) => call<string>('agents_md_draft', { name }),
  /// 票 16：空目录的一句话 → 项目说明草稿。不写磁盘。
  optimizeAgentsMd: (name: string, sentence: string) =>
    call<string>('optimize_agents_md', { name, sentence }),
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
      fastpathRole?: string | null
      initGit: boolean
      agentsMd?: string | null
      autonomy?: string | null
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
        fastpathRole: opts.fastpathRole ?? null,
        initGit: opts.initGit,
        agentsMd: opts.agentsMd ?? null,
        autonomy: opts.autonomy ?? null,
      },
    }
    if (!isTauri) {
      // 浏览器 dev 没有壳层通道。按核的顺序当场报告再返回，不另睡假装耗时。
      for (const step of CREATE_STEPS) onStep?.(step)
      return call<void>('create_project', args)
    }
    const onProgress = new Channel<CreateStep>()
    onProgress.onmessage = (step) => onStep?.(step)
    return invoke<void>('create_project', { ...args, onProgress })
  },
}

// ---- 浏览器 dev mock：覆盖全部事件 kind / 全部待决卡 / 产物版本链 / 代码文件 ----
// a1（架构师）是「完整 agent」样板：回合、工具组、消息、交付、权限、复审、打回、提案全有。

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
  message: { id, author, body, tokens: [], attachments: [], created_at: `${T0}${hhmm}:00Z`, thinking: '' },
})

const ART_CONTENT: Record<string, Record<number, string>> = {
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

function mock<T>(cmd: string, args?: Record<string, unknown>): T {
  switch (cmd) {
    case 'core_ping':
      return 'hexagon-core ok' as T
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
        { id: 'a0', role: '产品策划', status: 'active', model_slot: 'chat' },
        { id: 'a1', role: '架构师', status: 'active', model_slot: 'chat' },
        { id: 'a2', role: 'UX', status: 'sleeping', model_slot: 'chat' },
        { id: 'a3', role: '前端', status: 'active', model_slot: 'code' },
        { id: 'a4', role: '后端', status: 'active', model_slot: 'code' },
        { id: 'a5', role: 'QA', status: 'active', model_slot: 'chat' },
        { id: 'a6', role: '后端技术负责人', status: 'sleeping', model_slot: 'chat' },
        { id: 'a7', role: '运维', status: 'sleeping', model_slot: 'ops' },
      ].map((r) => ({
        ...r,
        avatar_hash: mockAvatars[r.id] ? mockHash(mockAvatars[r.id]) : null,
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
      ] as T
    case 'timeline': {
      // mock 也尊重 after/limit（票 07 增量通道）：真实后端同语义，
      // dev/测试里才能演练游标归并。
      const all: TimelineItem[] = [
        mkEv(1, 'pack_upgraded', null, null, { pack: '规格驱动' }, '09:00'),
        mkEv(2, 'agent_activated', 'a0', null, {}, '09:00'),
        mkEv(3, 'system', null, null, { note: '快速通道项目钉包副本，切规格驱动 v1' }, '09:00'),

        mkEv(4, 'stage_started', null, 'r0', { stage: '需求', run_id: 'r0' }, '09:01'),
        mkEv(5, 'turn_started', 'a0', 'r0', {}, '09:02'),
        mkEv(6, 'tool_called', 'a0', 'r0', { tool: 'fs.read', path: 'docs/brief.md' }, '09:02'),
        mkEv(7, 'tool_result', 'a0', 'r0', { tool: 'fs.read', ok: true }, '09:02'),
        mkEv(8, 'tool_called', 'a0', 'r0', { tool: 'fs.write', path: 'specs/prd.md' }, '09:03'),
        mkEv(9, 'tool_result', 'a0', 'r0', { tool: 'fs.write', ok: true }, '09:03'),
        mkMsg(10, 'agent_message', 'a0', 'r0', 'a0', '需求梳理完：聚焦家庭一周菜单。规格 v1 落 `specs/prd.md`，验收三条。', '09:05'),
        mkEv(11, 'artifact_delivered', 'a0', 'r0', { path: 'specs/prd.md', kind: '规格', version: 1, status: 'valid' }, '09:05'),
        mkEv(12, 'review_passed', 'a1', 'r0', { path: 'specs/prd.md' }, '09:07'),
        mkEv(13, 'turn_finished', 'a0', 'r0', {}, '09:08'),
        mkEv(14, 'stamped', null, 'r0', { stage: '需求' }, '09:10'),
        mkEv(15, 'stage_finished', null, 'r0', { stage: '需求' }, '09:10'),

        mkEv(16, 'stage_started', null, 'r1', { stage: '界面稿', run_id: 'r1' }, '09:11'),
        mkEv(17, 'agent_activated', 'a2', 'r1', {}, '09:11'),
        mkEv(18, 'turn_started', 'a2', 'r1', {}, '09:12'),
        mkEv(19, 'tool_called', 'a2', 'r1', { tool: 'fs.write', path: 'ui/screens.md' }, '09:13'),
        mkEv(20, 'tool_result', 'a2', 'r1', { tool: 'fs.write', ok: true }, '09:13'),
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
        mkEv(35, 'tool_called', 'a1', 'r2', { tool: 'fs.read', path: 'specs/prd.md' }, '09:31'),
        mkEv(36, 'tool_result', 'a1', 'r2', { tool: 'fs.read', ok: true }, '09:31'),
        mkEv(37, 'tool_called', 'a1', 'r2', { tool: 'fs.write', path: 'api/spec.md' }, '09:32'),
        mkEv(38, 'tool_result', 'a1', 'r2', { tool: 'fs.write', ok: true }, '09:32'),
        mkEv(39, 'permission_asked', 'a1', 'r2', { tool: 'bash', input: { command: 'sqlite3 .hexagon/state.db .schema' }, reason: '核对事件表结构' }, '09:33'),
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
        mkEv(56, 'tool_called', 'a3', 'r3', { tool: 'fs.write', path: 'src/recipes.ts' }, '10:04'),
        mkEv(57, 'tool_result', 'a3', 'r3', { tool: 'fs.write', ok: true }, '10:04'),
        mkEv(58, 'tool_called', 'a3', 'r3', { tool: 'bash', input: { command: 'npm run dev' } }, '10:05'),
        mkEv(59, 'tool_result', 'a3', 'r3', { tool: 'bash', ok: true }, '10:05'),
        mkEv(60, 'artifact_delivered', 'a3', 'r3', { path: 'src/recipes.ts', kind: '代码', version: 1, status: 'valid' }, '10:07'),
        mkMsg(61, 'agent_message', 'a3', 'r3', 'a3', '列表页能跑了：筛选、标签 chip、加载态。', '10:07'),
        mkEv(62, 'turn_finished', 'a3', 'r3', {}, '10:08'),
        mkEv(63, 'turn_started', 'a4', 'r3', {}, '10:10'),
        mkEv(64, 'tool_called', 'a4', 'r3', { tool: 'fs.write', path: 'crates/core/src/api.rs' }, '10:11'),
        mkEv(65, 'tool_result', 'a4', 'r3', { tool: 'fs.write', ok: true }, '10:11'),
        mkEv(66, 'tool_called', 'a4', 'r3', { tool: 'bash', input: { command: 'cargo build' } }, '10:12'),
        mkEv(67, 'tool_result', 'a4', 'r3', { tool: 'bash', ok: true }, '10:12'),
        mkEv(68, 'artifact_delivered', 'a4', 'r3', { path: 'crates/core/src/api.rs', kind: '代码', version: 1, status: 'valid' }, '10:14'),
        mkMsg(69, 'agent_message', 'a4', 'r3', 'a4', '核 API 接缝落完：`timeline()` + 事件行投影。', '10:14'),
        mkMsg(70, 'owner_message', null, null, 'owner', '先补列表页单测，再推联调。', '10:16'),
        mkEv(71, 'turn_failed', 'a4', 'r3', { error: '模型供应商超时，已自动重试' }, '10:20'),
        mkEv(72, 'permission_asked', 'a4', 'r3', { tool: 'bash', input: { command: 'rm -rf node_modules && npm i' }, reason: '依赖树损坏需重装' }, '10:22'),
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
        mkEv(92, 'publish_requested', null, null, { remote: 'origin', baseline: 'main' }, '11:08'),
        mkEv(93, 'publish_failed', null, null, { remote: 'origin', error: '网络超时' }, '11:09'),
        mkEv(94, 'publish_requested', null, null, { remote: 'origin', baseline: 'main' }, '11:15'),
        mkEv(95, 'publish_confirmed', null, null, { remote: 'origin', baseline: 'main' }, '11:16'),
        mkEv(96, 'baseline_merged', null, null, { baseline: 'main', commit: 'c4f9a2e' }, '11:17'),
        mkEv(97, 'publish_requested', null, null, { remote: 'upstream', baseline: 'main' }, '11:20'),
        mkEv(98, 'publish_rejected', null, null, { remote: 'upstream', by: 'owner' }, '11:21'),
        mkEv(99, 'usage_cap_hit', null, null, { spent_mc: 40200, limit_cents: 20000 }, '11:25'),
        mkEv(100, 'team_slept', null, null, {}, '11:25'),
        mkEv(101, 'agent_slept', 'a3', null, {}, '11:25'),
        mkMsg(102, 'agent_message', 'a1', 'r3', 'a1', '今日收口：实现待盖章，`f12` 已升级待裁决，origin/main 已发布。', '11:30'),
        mkEv(103, 'fastpath_dispatched', null, null, { role: '运维', input: '准备部署清单' }, '11:32'),
        mkEv(104, 'return_summary', null, null, {
          events: 38, pending_todos: 6,
          artifacts: { valid: 4, stamped: 4, pending: 1 },
          flags: { open: 1 }, permissions: { asked: 1 }, stages: { waiting_stamp: 1 },
        }, '11:35'),
      ]
      const after = args?.after == null ? null : Number(args.after)
      const limit = Number(args?.limit ?? 500)
      return all.filter((i) => after == null || i.event.id > after).slice(0, limit) as T
    }
    case 'pending_questions':
      return [
        { id: 'q-rec', kind: 'recovery', agent_id: 'a1', payload: { run_id: 'r9', stage: '部署演练' }, state: 'queued' },
        { id: 'q-pub', kind: 'publish', agent_id: 'a0', payload: { remote: 'origin', baseline: 'main', warning: '不可逆：代码与产物将离开本机' }, state: 'queued' },
        { id: 'q-stamp', kind: 'stamp', agent_id: 'a1', payload: { run_id: 'r3', stage: '实现' }, state: 'queued' },
        { id: 'q-esc', kind: 'escalation', agent_id: 'a1', payload: { flag_id: 'f12', target: 'api/spec.md' }, state: 'queued' },
        { id: 'q-perm', kind: 'permission', agent_id: 'a0', payload: { tool: 'bash', input: { command: 'cargo test --workspace' }, reason: '检验前全量回归', safety_net: false }, state: 'queued' },
        { id: 'q-perm2', kind: 'permission', agent_id: 'a1', payload: { tool: 'bash', input: { command: 'rm -rf target && cargo build' }, reason: '构建产物损坏，安全网必问', safety_net: true }, state: 'queued' },
        { id: 'q-prop', kind: 'stamp', agent_id: 'a1', payload: { proposal_id: 'p2', surface: '工具白名单', warnings: ['改动基线权限'], warning_text: '改动基线权限' }, state: 'queued' },
      ] as T
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
      return [
        mk('2026-09-15', 'a0', 5200, 1400, 0, 2400),
        mk('2026-09-15', 'a1', 8800, 2900, 600, 5600),
        mk('2026-09-16', 'a0', 6900, 1500, 0, 2800),
        mk('2026-09-16', 'a1', 19400, 6300, 900, 11400),
        mk('2026-09-16', 'a2', 12400, 4800, 0, 6800),
        mk('2026-09-17', 'a1', 9900, 3100, 500, 6200),
        mk('2026-09-17', 'a3', 18200, 7200, 2600, 10400),
        mk('2026-09-18', 'a1', 32800, 11200, 1500, 13800),
        mk('2026-09-18', 'a3', 21600, 8100, 3400, 12600),
        mk('2026-09-18', 'a4', 16800, 5900, 2200, 9200),
        mk('2026-09-18', 'a5', 6400, 1800, 0, 2900),
      ] as T
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
      } as T
    case 'log_enabled':
      return true as T
    case 'set_agent_avatar':
      mockAvatars[String(args?.agentId)] = String(args?.dataUrl)
      return null as T
    case 'agent_avatar':
      return (mockAvatars[String(args?.agentId)] ?? null) as T
    case 'artifact_content':
      return artLatest(String(args?.path)) as T
    case 'artifact_content_at':
      return (ART_CONTENT[String(args?.path)]?.[Number(args?.version)] ?? null) as T
    case 'set_agent_sleeping':
    case 'review_proposal':
    case 'confirm_proposal':
    case 'reject_proposal':
    case 'rollback_proposal':
    case 'request_publish':
    case 'confirm_publish':
    case 'reject_publish':
    case 'reject_stamp':
    case 'adjudicate_flag':
    case 'answer_permission':
    case 'stamp':
    case 'sleep_all':
    case 'send_message':
      return null as T
    case 'proposals':
      return [
        { id: 'p2', artifact_path: 'proposals/p2.md', surface: 'pack_copy', target: 'grants', status: 'awaiting_stamp', author: 'a4' },
        // 票 08：in_review 待负责人裁决（复审 agent 无工具面）
        { id: 'p3', artifact_path: 'proposals/p3.md', surface: 'agents_md', target: 'AGENTS.md', status: 'in_review', author: 'a2' },
      ] as T
    case 'mcp_services':
      return [] as T
    case 'list_mcp_entries':
      return [
        { name: 'termius', command: 'ssh-mcp', args: ['--stdio'], env: {}, cwd: null, disabled: false, transport: 'stdio', url: null, origin: 'global' },
        { name: 'web-svc', command: '', args: [], env: {}, cwd: null, disabled: true, transport: 'remote', url: 'https://h/sse', origin: 'global' },
      ] as T
    case 'save_mcp_service':
    case 'delete_mcp_service':
    case 'open_mcp_market':
      return null as T
    case 'scan_external_mcp':
      return [
        { name: 'figma', command: 'npx', args: ['-y', 'figma-mcp'], env: {}, cwd: null, disabled: false, transport: 'stdio', url: null, origin: 'cursor', source_path: '~/.cursor/mcp.json', conflict: false },
        { name: 'web-svc', command: '', args: [], env: {}, cwd: null, disabled: false, transport: 'remote', url: 'https://h/sse', origin: 'claude', source_path: '~/.claude.json', conflict: true },
      ] as T
    case 'import_mcp':
      return { imported: 1, skipped: ['web-svc: conflict'] } as T
    case 'repo_paths':
      return ['src/', 'src/api.rs', 'docs/', 'AGENTS.md'] as T
    case 'list_repo_dir': {
      const rel = String(args?.rel ?? '')
      if (rel === '' || rel === '.') {
        return [
          { name: 'src', path: 'src', kind: 'dir' },
          { name: 'README.md', path: 'README.md', kind: 'file' },
          { name: 'package.json', path: 'package.json', kind: 'file' },
        ] as T
      }
      if (rel === 'src') {
        return [{ name: 'main.ts', path: 'src/main.ts', kind: 'file' }] as T
      }
      return [] as T
    }
    case 'read_repo_file':
      return `// ${args?.path}\n` as T
    case 'write_repo_file':
    case 'create_repo_file':
    case 'create_repo_dir':
      return null as T
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
      return true as T
    case 'recent_projects':
      return [] as T
    case 'open_recent':
    case 'close_project':
      return null as T
    case 'project_info':
      return { name: '食谱 App', mode: 'pack', pack_name: '规格驱动', fastpath_agent_id: null, fastpath_role: null } as T
    case 'dispatch':
    case 'upgrade_to_pack':
      return null as T
    case 'inspect_dir':
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
    case 'agent_detail':
      return {
        agent_id: 'a0', role: '后端', status: 'active', model_slot: 'chat', custom: false,
        def: { duty: '服务端实现', reviewer: '后端技术负责人', model_slot: 'chat', skills: [] },
        globs: ['src/**'], grants: [{ kind: 'skill', name: 'spec-writing' }],
      } as T
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
      return {
        providers: [
          {
            id: 'openrouter', name: 'OpenRouter', kind: 'openai',
            base_url: 'https://openrouter.ai/api/v1', enabled: true, key_set: true,
            models: [
              { id: 'deepseek/deepseek-chat', caps: ['tools'] },
              { id: 'google/gemini-2.5-flash', caps: ['vision', 'tools'] },
              { id: 'deepseek/deepseek-r1:free', caps: ['free', 'reasoning', 'tools'] },
            ],
          },
          {
            id: 'anthropic', name: 'Anthropic', kind: 'anthropic',
            base_url: 'https://api.anthropic.com', enabled: false, key_set: false, models: [],
          },
        ],
        slots: { chat: { provider_id: 'openrouter', model: 'deepseek/deepseek-chat' } },
      } as T
    case 'save_provider':
    case 'delete_provider':
    case 'set_slot_binding':
    case 'remove_slot_binding':
      return null as T
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
    default:
      return null as T
  }
}
