// 核 API 接缝：Tauri 环境走 invoke；浏览器开发环境用内置 mock 数据。
// UI 的唯一通道 = 这些命令 + 事件推送，没有旁路。类型对齐 api.rs 的 JSON 形状。

import { invoke } from '@tauri-apps/api/core'

export const isTauri = '__TAURI_INTERNALS__' in window || '__TAURI__' in window

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return invoke<T>(cmd, args)
  return mock<T>(cmd, args)
}

// ---- 与核侧 JSON 形状对齐 ----

export interface StageRow {
  run_id: string
  stage: string
  seq: number
  state: 'pending' | 'active' | 'done' | 'skipped' | 'waiting_stamp' | 'rejected'
}

export interface TimelineItem {
  event: {
    id: number
    kind: string // snake_case EventKind
    agent_id: string | null
    stage_run_id: string | null
    payload: Record<string, unknown>
    created_at: string
  }
  message: { id: number; author: string; body: string; tokens: unknown[] } | null
}

export interface PendingQuestion {
  id: string
  kind: string // permission / stamp / publish / proposal_confirm …
  payload: Record<string, unknown>
  state: string
}

export interface TeamRow {
  id: string
  role: string
  model_slot: string | null
  status: 'active' | 'sleeping'
}

export interface ArtifactRow {
  id: string
  path: string
  kind: string
  tier: string
  stage_run_id: string | null
  author: string | null
  version: number
  status: string
  upstream_id: string | null
}

export interface UsageTotal {
  _total: boolean
  spent_mc: number
  limit_cents: number | null
}

export interface UsageRow {
  _total?: boolean
  agent_id?: string | null
  model?: string | null
  stage?: string | null
  prompt_tokens?: number
  completion_tokens?: number
  tool_output_tokens?: number
  cost_mc?: number
  calls?: number
  spent_mc?: number
  limit_cents?: number | null
}

export interface UsagePoint {
  day: string
  cost_mc: number
}

export const api = {
  ping: () => call<string>('core_ping'),
  openProject: (dir: string, name: string, roles: [string, string][], packJson?: string) =>
    call<void>('open_project', { dir, name, roles, packJson: packJson ?? null }),
  timeline: (after?: number, limit = 500) =>
    call<TimelineItem[]>('timeline', { after: after ?? null, limit }),
  sendMessage: (body: string) => call<number>('send_message', { body }),
  answerPermission: (questionId: string, allow: boolean, rememberShape?: string, scope = 'activation') =>
    call<void>('answer_permission', { questionId, allow, rememberShape: rememberShape ?? null, scope }),
  advance: () => call<unknown>('advance'),
  openStage: (seq: number) => call<unknown>('open_stage', { seq }),
  runChecks: () => call<unknown>('run_checks'),
  stamp: () => call<unknown>('stamp'),
  rewind: (toSeq: number) => call<unknown>('rewind', { toSeq }),
  skip: () => call<unknown>('skip'),
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
  pendingQuestions: async (): Promise<PendingQuestion[]> => {
    const rows = await call<{ id: string; kind: string; payload: string; state: string }[]>(
      'pending_questions',
    )
    return rows.map((r) => ({ ...r, payload: JSON.parse(r.payload || '{}') }))
  },
  usage: () => call<UsageRow[]>('usage'),
  usageSeries: () => call<UsagePoint[]>('usage_series'),
  setUsageLimit: (limitCents: number | null) =>
    call<void>('set_usage_limit', { limitCents }),
  setLogEnabled: (enabled: boolean) => call<void>('set_log_enabled', { enabled }),
  logEnabled: () => call<boolean>('log_enabled'),
  autonomy: () => call<string>('autonomy'),
  setAutonomy: (level: string) => call<void>('set_autonomy', { level }),
  ownerAway: () => call<void>('owner_away'),
  ownerBack: () => call<unknown>('owner_back'),
  // ---- 决策卡动作 ----
  rejectStamp: () => call<unknown>('reject_stamp'),
  adjudicateFlag: (qid: string, agree: boolean) =>
    call<unknown>('adjudicate_flag', { qid, agree }),
  proposals: () => call<Record<string, unknown>[]>('proposals'),
  reviewProposal: (proposalId: string, pass: boolean, reason: string, reviewerAgent: string) =>
    call<void>('review_proposal', { proposalId, pass, reason, reviewerAgent }),
  confirmProposal: (qid: string) => call<string>('confirm_proposal', { qid }),
  rejectProposal: (qid: string, reason: string) =>
    call<void>('reject_proposal', { qid, reason }),
  rollbackProposal: (proposalId: string) =>
    call<void>('rollback_proposal', { proposalId }),
  requestPublish: (remote: string) => call<string>('request_publish', { remote }),
  confirmPublish: (qid: string) => call<unknown>('confirm_publish', { qid }),
  rejectPublish: (qid: string) => call<void>('reject_publish', { qid }),
  // ---- 头像 ----
  setAgentAvatar: (agentId: string, dataUrl: string) =>
    call<void>('set_agent_avatar', { agentId, dataUrl }),
  agentAvatar: (agentId: string) => call<string | null>('agent_avatar', { agentId }),
}

// ---- 浏览器 dev mock：参照 hexagon-main-mock.html 的场景，形状同核侧 ----
const mockAvatars: Record<string, string> = {}
function mock<T>(cmd: string, args?: Record<string, unknown>): T {
  switch (cmd) {
    case 'core_ping':
      return 'hexagon-core ok' as T
    case 'stage_status':
      return [
        { run_id: 'r0', stage: '需求', seq: 0, state: 'done' },
        { run_id: 'r1', stage: '界面稿', seq: 1, state: 'done' },
        { run_id: 'r2', stage: '接口', seq: 2, state: 'active' },
        { run_id: 'r3', stage: '实现', seq: 3, state: 'pending' },
      ] as T
    case 'team':
      return [
        { id: 'a0', role: '产品策划', status: 'active', model_slot: 'chat' },
        { id: 'a1', role: '架构师', status: 'active', model_slot: 'chat' },
        { id: 'a2', role: '前端', status: 'sleeping', model_slot: null },
      ] as T
    case 'artifacts':
      return [
        { id: 'art1', path: 'specs/prd.md', kind: '规格', tier: 'parse', stage_run_id: 'r0', author: 'a0', version: 2, status: 'stamped', upstream_id: null },
        { id: 'art2', path: 'ui/screens.md', kind: '界面稿', tier: 'header', stage_run_id: 'r1', author: 'a1', version: 1, status: 'superseded', upstream_id: null },
        { id: 'art3', path: 'ui/screens.md', kind: '界面稿', tier: 'header', stage_run_id: 'r1', author: 'a1', version: 2, status: 'valid', upstream_id: null },
      ] as T
    case 'timeline':
      return [
        { event: { id: 1, kind: 'stage_started', agent_id: null, stage_run_id: 'r2', payload: { stage: '接口' }, created_at: '2026-07-07T09:13:00Z' }, message: null },
        { event: { id: 2, kind: 'tool_called', agent_id: 'a1', stage_run_id: 'r2', payload: { tool: 'fs.read' }, created_at: '2026-07-07T09:16:00Z' }, message: null },
        { event: { id: 3, kind: 'tool_result', agent_id: 'a1', stage_run_id: 'r2', payload: {}, created_at: '2026-07-07T09:19:00Z' }, message: null },
        { event: { id: 4, kind: 'tool_called', agent_id: 'a1', stage_run_id: 'r2', payload: { tool: 'fs.write' }, created_at: '2026-07-07T09:22:00Z' }, message: null },
        { event: { id: 5, kind: 'agent_message', agent_id: 'a1', stage_run_id: 'r2', payload: {}, created_at: '2026-07-07T09:25:00Z' }, message: { id: 1, author: 'a1', body: '接口说明 v1 已交付，见产物。', tokens: [] } },
        { event: { id: 6, kind: 'artifact_delivered', agent_id: 'a1', stage_run_id: 'r2', payload: { path: 'api/spec.md', kind: '接口说明', version: 1 }, created_at: '2026-07-07T09:28:00Z' }, message: null },
        { event: { id: 7, kind: 'permission_asked', agent_id: 'a1', stage_run_id: 'r2', payload: {}, created_at: '2026-07-07T09:31:00Z' }, message: null },
      ] as T
    case 'pending_questions':
      return [
        { id: 'q-pub', kind: 'publish', payload: JSON.stringify({ remote: 'origin', baseline: 'main', warning: 'Irreversible: code and artifacts leave the machine' }), state: 'queued' },
        { id: 'q-perm', kind: 'permission', payload: JSON.stringify({ tool: 'bash', input: { command: 'cargo test' }, reason: 'run test suite', safety_net: false }), state: 'queued' },
      ] as T
    case 'usage':
      return [
        { agent_id: 'a0', model: 'mock-chat', stage: '接口', prompt_tokens: 18000, completion_tokens: 4200, tool_output_tokens: 0, cost_mc: 7400, calls: 8 },
        { agent_id: 'a0', model: 'mock-chat', stage: '实现', prompt_tokens: 14000, completion_tokens: 3900, tool_output_tokens: 0, cost_mc: 5000, calls: 6 },
        { agent_id: 'a1', model: 'mock-chat', stage: '实现', prompt_tokens: 61000, completion_tokens: 20400, tool_output_tokens: 3000, cost_mc: 25800, calls: 22 },
        { _total: true, spent_mc: 38200, limit_cents: 20000 },
      ] as T
    case 'usage_series':
      return [
        { day: '2026-07-05', cost_mc: 8000 },
        { day: '2026-07-06', cost_mc: 14200 },
        { day: '2026-07-07', cost_mc: 16000 },
      ] as T
    case 'set_usage_limit':
      return null as T
    case 'autonomy':
      return 'L0' as T
    case 'log_enabled':
      return true as T
    case 'set_agent_avatar':
      mockAvatars[String(args?.agentId)] = String(args?.dataUrl)
      return null as T
    case 'agent_avatar':
      return (mockAvatars[String(args?.agentId)] ?? null) as T
    case 'artifact_content':
      return '# 接口说明 v1\n\nGET /api/recipes — 列表\nPOST /api/recipes — 新建\n' as T
    case 'artifact_content_at':
      return (Number(args?.version) <= 1
        ? '# 接口说明 v1\n\nGET /api/recipes\n'
        : '# 接口说明 v1\n\nGET /api/recipes — 列表\nPOST /api/recipes — 新建\n') as T
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
      return [] as T
    default:
      return null as T
  }
}
