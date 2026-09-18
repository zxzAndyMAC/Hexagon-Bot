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
  state: 'pending' | 'active' | 'done' | 'skipped' | 'waiting_stamp' | 'rejected' | 'interrupted'
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
  tokens?: number
}

export interface UsageBucket {
  bucket: string // "YYYY-MM-DD" | "YYYY-MM-DD HH:00"
  agent_id?: string | null
  prompt_tokens: number
  completion_tokens: number
  tool_output_tokens: number
  cost_mc: number
}

// ---- 项目向导（票 24）----
export interface DirReport {
  exists: boolean
  empty: boolean
  is_git: boolean
  dirty: boolean
  instructions: string | null // "AGENTS.md" | "CLAUDE.md" | null
}

export interface RoleDef {
  name: string
  duty: string
  reviewer: string | null
  model_slot: string
  globs: string[]
  skills: string[]
}

export interface PackDef {
  name: string
  version: number
  stages: { name: string; roles: string[]; due?: string[]; stamp_point?: boolean }[]
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
  pendingQuestions: async (): Promise<PendingQuestion[]> => {
    const rows = await call<{ id: string; kind: string; payload: string; state: string }[]>(
      'pending_questions',
    )
    return rows.map((r) => ({ ...r, payload: JSON.parse(r.payload || '{}') }))
  },
  usage: () => call<UsageRow[]>('usage'),
  usageSeries: (granularity: 'day' | 'hour' = 'day', from?: string | null, to?: string | null) =>
    call<UsageBucket[]>('usage_series', { granularity, from: from ?? null, to: to ?? null }),
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
  // ---- 崩溃恢复（票 37）----
  recoverRun: (runId: string) => call<void>('recover_run', { runId }),
  // ---- 检验覆盖（票 40）：显式覆盖留痕，composer /override <理由> 同权 ----
  overrideChecks: (reason: string) => call<unknown>('override_checks', { reason }),
  // ---- 安装助手（票 36）：NL 请求 → 确认卡 → 负责人确认才执行；grants 永不动 ----
  requestInstall: (desc: string) => call<string>('request_install', { desc }),
  resolveInstall: (qid: string, allow: boolean) =>
    call<unknown>('resolve_install', { qid, allow }),
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
  recentProjects: () =>
    call<{ dir: string; name: string; mode: string; opened_at: number }[]>('recent_projects'),
  openRecent: (dir: string) => call<void>('open_recent', { dir }),
  closeProject: () => call<void>('close_project'),
  // ---- 快速通道（票 26）----
  projectInfo: () =>
    call<{
      name: string
      mode: 'pack' | 'fastpath'
      pack_name: string | null
      fastpath_agent_id: string | null
      fastpath_role: string | null
    }>('project_info'),
  dispatch: (role: string, input: string) => call<unknown>('dispatch', { role, input }),
  upgradeToPack: (packName: string) => call<void>('upgrade_to_pack', { packName }),
  // ---- 项目向导（票 24）----
  projectOpen: () => call<boolean>('project_open'),
  inspectDir: (dir: string) => call<DirReport>('inspect_dir', { dir }),
  presetRoles: () => call<RoleDef[]>('preset_roles'),
  presetPacks: () => call<PackDef[]>('preset_packs'),
  checkModelKeys: (slots: string[]) => call<string[]>('check_model_keys', { slots }),
  setModelKey: (slot: string, secret: string) =>
    call<void>('set_model_key', { slot, secret }),
  agentsMdDraft: (name: string) => call<string>('agents_md_draft', { name }),
  createProject: (opts: {
    dir: string
    name: string
    roles: string[]
    packName?: string | null
    fastpathRole?: string | null
    initGit: boolean
    agentsMd?: string | null
  }) =>
    call<void>('create_project', {
      opts: {
        dir: opts.dir,
        name: opts.name,
        roles: opts.roles,
        packName: opts.packName ?? null,
        fastpathRole: opts.fastpathRole ?? null,
        initGit: opts.initGit,
        agentsMd: opts.agentsMd ?? null,
      },
    }),
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
type Payload = Record<string, unknown>
const mkEv = (
  id: number, kind: string, agent_id: string | null, stage_run_id: string | null,
  payload: Payload, hhmm: string,
): TimelineItem => ({
  event: { id, kind, agent_id, stage_run_id, payload, created_at: `${T0}${hhmm}:00Z` },
  message: null,
})
const mkMsg = (
  id: number, kind: string, agent_id: string | null, stage_run_id: string | null,
  author: string, body: string, hhmm: string,
): TimelineItem => ({
  event: { id, kind, agent_id, stage_run_id, payload: {}, created_at: `${T0}${hhmm}:00Z` },
  message: { id, author, body, tokens: [] },
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
      ] as T
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
    case 'timeline':
      return [
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
      ] as T
    case 'pending_questions':
      return [
        { id: 'q-rec', kind: 'recovery', payload: JSON.stringify({ run_id: 'r9', stage: '部署演练' }), state: 'queued' },
        { id: 'q-pub', kind: 'publish', payload: JSON.stringify({ remote: 'origin', baseline: 'main', warning: '不可逆：代码与产物将离开本机' }), state: 'queued' },
        { id: 'q-stamp', kind: 'stamp', payload: JSON.stringify({ run_id: 'r3', stage: '实现' }), state: 'queued' },
        { id: 'q-esc', kind: 'escalation', payload: JSON.stringify({ flag_id: 'f12', target: 'api/spec.md' }), state: 'queued' },
        { id: 'q-perm', kind: 'permission', payload: JSON.stringify({ tool: 'bash', input: { command: 'cargo test --workspace' }, reason: '检验前全量回归', safety_net: false }), state: 'queued' },
        { id: 'q-perm2', kind: 'permission', payload: JSON.stringify({ tool: 'bash', input: { command: 'rm -rf target && cargo build' }, reason: '构建产物损坏，安全网必问', safety_net: true }), state: 'queued' },
        { id: 'q-prop', kind: 'stamp', payload: JSON.stringify({ proposal_id: 'p2', surface: '工具白名单', warnings: ['改动基线权限'], warning_text: '改动基线权限' }), state: 'queued' },
      ] as T
    case 'usage':
      return [
        { agent_id: 'a0', model: 'mock-chat', stage: '需求', prompt_tokens: 18000, completion_tokens: 4200, tool_output_tokens: 0, cost_mc: 7400, calls: 8 },
        { agent_id: 'a1', model: 'mock-chat', stage: '接口', prompt_tokens: 42000, completion_tokens: 12800, tool_output_tokens: 2100, cost_mc: 19600, calls: 17 },
        { agent_id: 'a1', model: 'mock-chat', stage: '实现', prompt_tokens: 19000, completion_tokens: 5600, tool_output_tokens: 900, cost_mc: 8200, calls: 7 },
        { agent_id: 'a2', model: 'mock-chat', stage: '界面稿', prompt_tokens: 22000, completion_tokens: 9100, tool_output_tokens: 0, cost_mc: 10400, calls: 9 },
        { agent_id: 'a3', model: 'mock-code', stage: '实现', prompt_tokens: 36000, completion_tokens: 14200, tool_output_tokens: 3200, cost_mc: 17800, calls: 14 },
        { agent_id: 'a4', model: 'mock-code', stage: '实现', prompt_tokens: 28000, completion_tokens: 9800, tool_output_tokens: 2600, cost_mc: 13500, calls: 11 },
        { agent_id: 'a5', model: 'mock-chat', stage: '实现', prompt_tokens: 6400, completion_tokens: 1800, tool_output_tokens: 0, cost_mc: 2900, calls: 3 },
        { _total: true, spent_mc: 81400, limit_cents: 20000, tokens: 286400 },
      ] as T
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
      return 'L1' as T
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
        { id: 'p2', artifact_path: 'proposals/p2.md', surface: '工具白名单', status: 'queued' },
      ] as T
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
      return { exists: true, empty: false, is_git: true, dirty: false, instructions: 'AGENTS.md' } as T
    case 'preset_roles':
      return [
        { name: '产品策划', duty: '需求与规格', reviewer: null, model_slot: 'chat', globs: [], skills: ['spec-writing'] },
        { name: '后端', duty: '服务端实现', reviewer: '后端技术负责人', model_slot: 'chat', globs: [], skills: [] },
      ] as T
    case 'preset_packs':
      return [
        { name: '规格驱动', version: 1, stages: [{ name: '规格', roles: ['产品策划'], due: ['规格'], stamp_point: true }] },
      ] as T
    case 'check_model_keys':
      return [] as T
    case 'export_events':
      return 42 as T // mock：导出条数
    case 'request_install':
      return 'q-mock-install' as T // mock：待决卡 id
    case 'resolve_install':
      return { installed: args?.allow === true } as T
    case 'set_model_key':
    case 'create_project':
      return null as T
    case 'agents_md_draft':
      return `# ${args?.name ?? 'project'}\n\n## Commands\n` as T
    default:
      return null as T
  }
}
