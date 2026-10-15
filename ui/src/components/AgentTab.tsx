// Agent 详情 AgentDetail（components D16，canvas#agent；interaction §6「Agent」）。ui-redesign 票 09 换皮。
// 头卡：40 头像（点击换图）+ 名 15/600 + 「职责 · 上级」12.5 ink-2 + 右侧「休眠」default / 休眠时「唤醒」primary；
// 第二行 cpu 胶囊模型 + 状态胶囊（工作中 / 休眠 / 空闲）。→ A7「活动 / 职责 / 用量」。
// 活动 = Task Rows（agentTasks.ts 分组：一行一个回合，状态圆 + 粗标题 + 灰 meta + 结果胶囊 + ▾，展开子步骤左竖线缩进，右侧等宽时间）；
// 子步骤仍可展开看原始负载 / 执行明细（ExecBody 同源复用）。底部用量两行：上下文进度（A12）+ 本项目花费。
// 原型的「出图：未配置」胶囊不画：核心没有出图槽位的读模型（reconciliation 票 09）。
import { useLayoutEffect, useMemo, useRef, useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type AgentDetail } from '../api'
import { useUiStore } from '../store'
import { CodeBlock, Md } from './Md'
import { Icon, type IconName } from './Icon'
import { RoleEditor } from './RoleEditor'
import { fmtTime as fmtTimeShared, fmtYuan } from '../usage'
import { pairToolCalls, toolOutcome, type ToolOutcome, toolInputSummary, TOOL_ICON, TOOL_LABEL, EXEC_CARD_TOOLS, type ToolCall } from '../agentSteps'
import { groupTasks, type TaskGroup } from '../agentTasks'
import { factOpenTurns } from '../timelineFacts'
import { slotLabel } from '../modelpick'
import { useTimelineWindow } from '../useTimelineWindow'
import { bindingFor, formatBinding, matches } from '../keymap'
import { ExecBody, CallStatus } from './ExecCard'
import { Avatar } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dot } from '@/components/ui/dot'
import { Progress } from '@/components/ui/progress'
import { Spinner } from '@/components/ui/spinner'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { cn } from '@/lib/utils'

type Step = {
  id: number
  kind: string
  icon: IconName
  label: string
  summary?: string
  detail?: string
  ok?: ToolOutcome
  message?: string
  path?: string
  divider?: boolean
  time: string
  // exec-cards 票 02：重负载工具的调用对——展开明细借 ExecBody（同源复用）。
  execCall?: ToolCall
}

const STEP_ICON: Record<string, IconName> = {
  turn_finished: 'check', turn_failed: 'warn', agent_activated: 'bolt', agent_slept: 'sleep', consult_wakeup: 'sleep',
  stage_finished: 'stamp', artifact_delivered: 'artifact', artifact_rejected: 'artifact', review_passed: 'check',
  review_rejected: 'warn', review_skipped: 'arrow-right', flag_submitted: 'flag', flag_adjudicated: 'flag',
  permission_asked: 'help', permission_allowed: 'check', permission_denied: 'close', permission_shape_remembered: 'check',
  escalated: 'escalate', test_ran: 'check', proposal_queued: 'diff', proposal_reviewed: 'diff', proposal_stamped: 'stamp',
  proposal_rejected: 'diff', proposal_activated: 'bolt', proposal_rolled_back: 'arrow-left', publish_requested: 'publish',
  publish_confirmed: 'publish', publish_rejected: 'publish', publish_failed: 'publish', backfill_executed: 'refresh',
  usage_cap_hit: 'yen', owner_command: 'send', fastpath_dispatched: 'bolt', system: 'list',
}

// 票 14：fmtTime 收敛到 usage.ts（Intl 本地语序）；本页要秒 → withSeconds。
const fmtTime = (iso: string) => fmtTimeShared(iso, true)
const fmtK = (n: number) => (n >= 1000 ? `${Math.round(n / 1000)}K` : String(n))

type Page = 'activity' | 'duty' | 'usage'

export function AgentTab({ agentId }: { agentId: string }) {
  const { t } = useTranslation()
  const { team, timelineFacts, invalidate, pushToast } = useUiStore()
  const providers = useUiStore((s) => s.providers)
  const avatarUrl = useUiStore((s) => s.avatars[agentId])
  const member = team.find((m) => m.id === agentId)
  const fileRef = useRef<HTMLInputElement>(null)
  const [page, setPage] = useState<Page>('activity')
  const [detail, setDetail] = useState<AgentDetail | null>(null)
  useEffect(() => {
    let live = true
    api.agentDetail(agentId).then((d) => { if (live) setDetail(d) }).catch(() => { if (live) setDetail(null) })
    return () => { live = false }
  }, [agentId, member?.status, member?.model_slot])

  // 忙碌 = 该 agent 有未收束回合（beautiful-ui 票 04）。openTurns per-agent——
  // 并发回合交错时别家的开窗不能遮本家的忙碌态。
  const streams = useUiStore((s) => s.streams[agentId])
  const thinking = useUiStore((s) => s.thinkings[agentId])
  const done = useUiStore((s) => s.streamDone[agentId])
  const turns = useMemo(() => factOpenTurns(timelineFacts), [timelineFacts])
  const busy = turns.has(agentId) || (done == null &&
    [streams, thinking].some((buffer) => buffer && Object.values(buffer).some(Boolean)))

  if (!member) return <div className="p-3.5 text-[12.5px] text-ink-3">{agentId}</div>

  const sleeping = member.status === 'sleeping'
  const duty = detail?.def.duty?.split('\n').find((l) => l.trim())?.trim()
  const reviewer = detail?.def.reviewer
  const reviewerName = reviewer ? team.find((m) => m.id === reviewer || m.role === reviewer)?.role ?? reviewer : null
  const sub = [duty, reviewerName ? t('agentDetail.reviewer', { who: reviewerName }) : null].filter(Boolean).join(' · ')

  const pickAvatar = (f: File) => {
    // ui-audit 票 04（P1-6）：>2MB 本地拒绝不发请求（data URL 会膨胀 ~4/3）；上传失败走 toast。
    if (f.size > 2 * 1024 * 1024) { pushToast(t('agent.avatarTooBig'), 'err'); return }
    // Benchmark I1 / 2026-10-06 Spec: a delayed A FileReader previously
    // fetched B's IPC identity and wrote B's same-named agent avatar.
    const epoch = useUiStore.getState().projectEpoch
    const r = new FileReader()
    r.onload = async () => {
      if (useUiStore.getState().projectEpoch !== epoch) return
      try {
        await api.setAgentAvatar(agentId, String(r.result))
        if (useUiStore.getState().projectEpoch === epoch) await invalidate('team')
      } catch (err) {
        if (useUiStore.getState().projectEpoch === epoch) pushToast(errText(err), 'err')
      }
    }
    r.readAsDataURL(f)
  }

  return (
    <div data-agent-detail className="flex min-h-0 flex-1 flex-col">
      {/* 头卡 */}
      <div className="flex-none px-[18px] pt-[18px]">
        <div className="flex items-center gap-3.5">
          <Avatar agentId={agentId} name={member.role} size={40} src={avatarUrl}
            onChangeImage={() => fileRef.current?.click()} changeImageLabel={t('agent.changeAvatar')} />
          <input ref={fileRef} type="file" accept="image/png,image/jpeg,image/webp,image/gif" className="hidden"
            onChange={(e) => { const f = e.target.files?.[0]; if (f) pickAvatar(f); e.target.value = '' }} />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[15px] font-semibold text-ink">{member.role}</div>
            {sub && <div className="truncate text-[12.5px] text-ink-2" title={sub}>{sub}</div>}
          </div>
          <Button size="sm" variant={sleeping ? 'primary' : 'default'} data-agent-sleep
            onClick={async () => {
              try { await api.setAgentSleeping(agentId, !sleeping); await invalidate('team') }
              catch (err) { pushToast(errText(err), 'err') }
            }}>
            <Icon name={sleeping ? 'bolt' : 'sleep'} size={12} />{sleeping ? t('agent.wake') : t('agent.sleep')}
          </Button>
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-1.5">
          <Badge data-agent-model><Icon name="cpu" size={12} />{slotLabel(member.model_slot, providers, t('agent.dedicatedTag'))}</Badge>
          {busy ? <Badge variant="ok" data-agent-state="busy"><Dot tone="on" />{t('timeline.working')}</Badge>
            : sleeping ? <Badge data-agent-state="sleeping"><Dot tone="off" />{t('side.sleeping')}</Badge>
            : <Badge data-agent-state="idle">{t('agentDetail.idle')}</Badge>}
        </div>
        <Segmented className="mt-4" value={page} onValueChange={(v) => setPage(v as Page)} aria-label={t('agentDetail.pages')}>
          <SegmentedItem value="activity">{t('agentDetail.activity')}</SegmentedItem>
          <SegmentedItem value="duty">{t('agentDetail.duty')}</SegmentedItem>
          <SegmentedItem value="usage">{t('agentDetail.usage')}</SegmentedItem>
        </Segmented>
      </div>
      {page === 'activity' && <Activity agentId={agentId} busy={busy} />}
      {page === 'duty' && <Duty agentId={agentId} detail={detail} reviewerName={reviewerName} />}
      {page === 'usage' && <UsageBreakdown agentId={agentId} />}
      <UsageFooter agentId={agentId} />
    </div>
  )
}

function Activity({ agentId, busy }: { agentId: string; busy: boolean }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const window = useTimelineWindow('all', agentId)
  const timeline = useMemo(() => window.page?.items ?? [], [window.page?.items])
  const scroll = useRef<HTMLDivElement>(null)
  const anchor = useRef<{ height: number; top: number } | null>(null)
  const loadOlder = () => {
    if (window.olderLoading || !window.page?.has_before) return
    if (scroll.current) anchor.current = { height: scroll.current.scrollHeight, top: scroll.current.scrollTop }
    void window.loadOlder()
  }
  useLayoutEffect(() => {
    if (window.change === 'prepend' && anchor.current && scroll.current) {
      scroll.current.scrollTop = anchor.current.top + scroll.current.scrollHeight - anchor.current.height
    }
    anchor.current = null
  }, [window.revision, window.change])

  const steps = useMemo(() => {
    const primary = timeline.filter((it) => it.event.agent_id === agentId)
    const pairs = pairToolCalls([...primary, ...(window.page?.boundary_pairs ?? [])].sort((a, b) => a.event.id - b.event.id))
    const calls = new Map(pairs.map(call => [call.called.event.id, call]))
    const visibleIds = new Set(primary.map(item => item.event.id))
    const orphanResults = new Map(pairs.filter(call => call.result && !visibleIds.has(call.called.event.id))
      .map(call => [call.result!.event.id, call.called]))
    // A result at the page boundary must still show its call; loading the prior
    // page replaces this proxy with the real call without a duplicate step.
    const items = primary.map(item => orphanResults.get(item.event.id) ?? item)
    const evLabel = (k: string) => t(`ev.${k}`, { defaultValue: k.replace(/_/g, ' ') })
    const out: Step[] = []
    for (const it of items) {
      const ev = it.event
      const p = ev.payload as Record<string, unknown>
      const base = { id: ev.id, kind: ev.kind, time: fmtTime(ev.created_at) }
      if (it.message) { out.push({ ...base, kind: 'agent_message', icon: 'agent', label: evLabel('agent_message'), message: it.message.body }); continue }
      if (ev.kind === 'turn_started') { out.push({ ...base, icon: 'bolt', label: `${evLabel('turn_started')} · ${String(p.stage ?? '')}`, divider: true }); continue }
      if (ev.kind === 'tool_called') {
        const tool = String(p.tool ?? '')
        const res = calls.get(ev.id)?.result
        out.push({
          ...base,
          icon: TOOL_ICON[tool] ?? 'tool',
          label: t(`agent.${TOOL_LABEL[tool] ?? 'stepTool'}`, { defaultValue: tool }),
          summary: toolInputSummary(p),
          detail: JSON.stringify({ ...p, ...(res ? { result: res.event.payload } : {}) }, null, 2),
          ok: toolOutcome(res),
          path: p.path ? String(p.path) : undefined,
          execCall: EXEC_CARD_TOOLS.has(tool) ? { called: it, result: res } : undefined,
        })
        continue
      }
      if (ev.kind === 'tool_result') continue // 已被 tool_called 吸收
      out.push({
        ...base,
        icon: STEP_ICON[ev.kind] ?? 'list',
        label: evLabel(ev.kind),
        summary: String(p.path ?? p.stage ?? p.flag_id ?? p.proposal_id ?? p.reason ?? p.tool ?? ''),
        detail: Object.keys(p).length ? JSON.stringify(p, null, 2) : undefined,
        path: ev.kind === 'artifact_delivered' ? String(p.path ?? '') : undefined,
      })
    }
    return out
  }, [timeline, window.page?.boundary_pairs, agentId, t])

  const groups = useMemo(() => groupTasks(steps, busy), [steps, busy])
  const [open, setOpen] = useState<Record<number, boolean>>({})
  const [openStep, setOpenStep] = useState<Set<number>>(new Set())
  const isOpen = (g: TaskGroup<Step>) => open[g.id] ?? g.state === 'running'
  const openPath = (path: string) => openTab({ id: `art:${path}`, kind: 'artifact', title: path, path })

  return (
    <div ref={scroll} data-agent-activity className="min-h-0 flex-1 overflow-y-auto px-[18px] pt-3 pb-3"
      onScroll={(e) => { if (e.currentTarget.scrollTop < 80) loadOlder() }}
      onKeyDown={(e) => {
        if (matches(e.nativeEvent, bindingFor('timelineLoadOlder'))) { e.preventDefault(); loadOlder() }
        else if (matches(e.nativeEvent, bindingFor('timelineRetry'))) { e.preventDefault(); void window.retry() }
      }}>
      {window.loading && <div className="flex items-center gap-2 py-2 text-[12.5px] text-ink-3"><Spinner size={12} />{t('timeline.loadingRecent')}</div>}
      {window.error && <Button size="xs" onClick={() => void window.retry()} title={[t('timeline.retry'), formatBinding(bindingFor('timelineRetry'))].filter(Boolean).join(' · ')}>{t('timeline.retry')}</Button>}
      {window.olderError && <p role="status" className="text-[12.5px] text-red">{t('timeline.olderFailed')}</p>}
      {window.olderLoading && <div className="flex items-center gap-2 py-2 text-[12.5px] text-ink-3"><Spinner size={12} />{t('timeline.loadingOlder')}</div>}
      {!window.olderLoading && window.page?.has_before && (
        <Button variant="ghost" size="xs" className="mb-2" onClick={loadOlder}
          title={[t('timeline.loadOlder'), formatBinding(bindingFor('timelineLoadOlder'))].filter(Boolean).join(' · ')}>
          {t(window.olderError ? 'timeline.retry' : 'timeline.loadOlder')}
        </Button>
      )}
      {!window.loading && !window.error && groups.length === 0 && <div className="py-2 text-[12.5px] text-ink-3">{t('agent.noEvents')}</div>}
      <div className="flex flex-col gap-1.5">
        {groups.map((g) => {
          const expanded = isOpen(g)
          const expandable = g.steps.length > 0
          const meta = g.calls > 0 ? t('agentDetail.calls', { count: g.calls }) : g.steps.length > 0 ? t('agentDetail.steps', { count: g.steps.length }) : g.time
          return (
            <div key={g.id} data-task={g.state} className="rounded-[12px] bg-surface shadow-[var(--shadow-card)]">
              <button type="button" aria-expanded={expandable ? expanded : undefined} disabled={!expandable}
                onClick={() => setOpen((o) => ({ ...o, [g.id]: !expanded }))}
                className={cn('hx-focus flex h-11 w-full items-center gap-2.5 rounded-[12px] px-3 text-left', expandable && 'cursor-pointer live:hover:bg-hover')}>
                <TaskCircle state={g.state} />
                <b className="min-w-0 flex-1 truncate text-[13px] font-semibold text-ink">{g.title}</b>
                <span className="flex-none text-[12px] text-ink-3">{meta}</span>
                {/* 回合外的工具调用：行上直接给调用结果（含「结果未知」，票 05 状态机） */}
                {g.step?.kind === 'tool_called' ? <CallStatus ok={g.step.ok} />
                  : g.state === 'failed' ? <Badge variant="err">{t('agentDetail.failed')}</Badge>
                  : g.delivered && g.state === 'done' ? <Badge variant="ok">{t('agentDetail.delivered')}</Badge> : null}
                {expandable && <Icon name={expanded ? 'chevron-up' : 'chevron-down'} size={12} className="flex-none text-ink-3" />}
              </button>
              {expandable && expanded && (
                <div className="mb-2.5 ml-[19px] flex flex-col gap-1 border-l-[1.5px] border-line pr-3 pl-[15px] text-[12px]">
                  {g.steps.map((s) => {
                    const stepOpen = openStep.has(s.id)
                    const toggle = () => setOpenStep((cur) => { const n = new Set(cur); if (n.has(s.id)) n.delete(s.id); else n.add(s.id); return n })
                    return (
                      <div key={s.id} data-step={s.kind}>
                        <div className="flex min-h-6 items-center gap-2">
                          <Icon name={s.icon} size={12} className="flex-none text-ink-3" />
                          <button type="button" className={cn('hx-focus min-w-0 truncate rounded-[4px] text-left text-ink-2', s.detail && 'cursor-pointer live:hover:text-ink')}
                            aria-expanded={s.detail ? stepOpen : undefined} onClick={() => { if (s.detail) toggle() }}>
                            {s.label}
                          </button>
                          {s.summary && (s.path
                            ? <button type="button" className="hx-focus min-w-0 truncate font-mono text-[11.5px] text-accent-ink live:hover:underline" title={s.path} onClick={() => openPath(s.path!)}>{s.summary}</button>
                            : <span className="min-w-0 truncate font-mono text-[11.5px] text-ink-3" title={s.summary}>{s.summary}</span>)}
                          <span className="flex-1" />
                          {/* 票 05 TaskRows 状态机：tool_called 无 result=在途运行环；落定翻 check/X */}
                          {s.kind === 'tool_called' && <CallStatus ok={s.ok} />}
                          <span className="flex-none font-mono text-ink-3">{s.time}</span>
                        </div>
                        {s.message && <div className="msg-body mt-1 text-[12.5px]"><Md>{s.message}</Md></div>}
                        {s.detail && stepOpen && (
                          <div className="my-1 overflow-x-auto rounded-[8px] text-[11px]">
                            {s.execCall ? <ExecBody call={s.execCall} /> : <CodeBlock code={s.detail} lang="json" />}
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

/** 状态圆（Task Rows）：完成绿实心勾 / 进行中环 / 失败红 x。 */
function TaskCircle({ state }: { state: TaskGroup['state'] }) {
  if (state === 'running') return <Spinner size={18} />
  if (state === 'unknown') return <span className="grid size-[18px] flex-none place-items-center rounded-full bg-orange text-white"><Icon name="help" size={12} /></span>
  return (
    <span className={cn('grid size-[18px] flex-none place-items-center rounded-full text-white', state === 'done' ? 'bg-green' : 'bg-red')}>
      <Icon name={state === 'done' ? 'check' : 'close'} size={12} />
    </span>
  )
}

function Duty({ agentId, detail, reviewerName }: { agentId: string; detail: AgentDetail | null; reviewerName: string | null }) {
  const { t } = useTranslation()
  const [editing, setEditing] = useState(false)
  return (
    <div data-agent-duty className="min-h-0 flex-1 overflow-y-auto px-[18px] pt-3 pb-3 text-[13px]">
      {detail == null ? <div className="flex items-center gap-2 text-ink-3"><Spinner size={12} />{t('file.loading')}</div> : (
        <>
          <div className="msg-body text-[13px]"><Md>{detail.def.duty || '—'}</Md></div>
          <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-[12.5px]">
            <dt className="text-ink-3">{t('agent.reviewer')}</dt><dd className="text-ink">{reviewerName ?? t('agent.noReviewer')}</dd>
            {detail.def.skills.length > 0 && (<><dt className="text-ink-3">{t('agentDetail.skills')}</dt>
              <dd className="flex flex-wrap gap-1">{detail.def.skills.map((s) => <Badge key={s} variant="line">{s}</Badge>)}</dd></>)}
          </dl>
        </>
      )}
      <Button size="xs" className="mt-3" onClick={() => setEditing((v) => !v)} aria-expanded={editing}>
        <Icon name="edit" size={12} />{t('agent.editRole')}
      </Button>
      {editing && <div className="mt-2"><RoleEditor agentId={agentId} onClose={() => setEditing(false)} /></div>}
    </div>
  )
}

/** 用量页：本 Agent 按模型 × 阶段的账（UsageRow 读模型，不新增 IPC）。 */
function UsageBreakdown({ agentId }: { agentId: string }) {
  const { t, i18n } = useTranslation()
  const rows = useUiStore((s) => s.usageRows).filter((r) => r.agent_id === agentId)
  return (
    <div data-agent-usage className="min-h-0 flex-1 overflow-y-auto px-[18px] pt-3 pb-3">
      {rows.length === 0 ? <div className="text-[12.5px] text-ink-3">{t('agentDetail.noUsage')}</div> : (
        <table className="w-full text-[12.5px]">
          <thead><tr className="text-left text-[12px] text-ink-3">
            <th className="py-1 font-normal">{t('agentDetail.model')}</th><th className="py-1 font-normal">{t('agentDetail.stage')}</th>
            <th className="py-1 text-right font-normal">{t('agentDetail.tokens')}</th><th className="py-1 text-right font-normal">{t('agentDetail.cost')}</th>
          </tr></thead>
          <tbody>
            {rows.map((r, i) => (
              <tr key={i} className="border-t border-line-soft">
                <td className="py-1.5 text-ink">{r.model ?? '—'}</td>
                <td className="py-1.5 text-ink-2">{r.stage ?? '—'}</td>
                <td className="py-1.5 text-right font-mono tabular-nums">{fmtK(r.prompt_tokens + r.completion_tokens + r.tool_output_tokens)}</td>
                <td className="py-1.5 text-right font-mono tabular-nums">{fmtYuan(r.cost_mc, i18n.language)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}

/** 底部用量两行（D16）：上下文 used / window（A12 accent）+ 本项目花费 · tokens。 */
function UsageFooter({ agentId }: { agentId: string }) {
  const { t, i18n } = useTranslation()
  const ctx = useUiStore((s) => s.timelineFacts?.latest_contexts.find((c) => c.agent_id === agentId)?.context ?? null)
  const rows = useUiStore((s) => s.usageRows)
  const spend = useMemo(() => rows.filter((r) => r.agent_id === agentId).reduce(
    (a, r) => ({ mc: a.mc + r.cost_mc, tokens: a.tokens + r.prompt_tokens + r.completion_tokens + r.tool_output_tokens }), { mc: 0, tokens: 0 }), [rows, agentId])
  const valid = ctx && ctx.window_tokens > 0
  return (
    <div data-agent-usage-footer className="mx-[18px] mb-[18px] flex-none rounded-[var(--r-card)] bg-inset p-3 shadow-[inset_0_0_0_1px_var(--line-soft)]">
      <div className="flex items-center text-[12px]">
        <span className="flex-1 text-ink-3">{t('agentDetail.context')}</span>
        <span className="font-mono tabular-nums">{valid ? `${fmtK(ctx.used_tokens)} / ${fmtK(ctx.window_tokens)}` : '—'}</span>
      </div>
      {valid && <Progress className="mt-2" tone="accent" value={Math.round((ctx.used_tokens / ctx.window_tokens) * 100)} aria-label={t('agentDetail.context')} />}
      <div className="mt-3 flex items-center text-[12px]">
        <span className="flex-1 text-ink-3">{t('agentDetail.spend')}</span>
        <span className="font-mono tabular-nums">{fmtYuan(spend.mc, i18n.language)} · {fmtK(spend.tokens)} tokens</span>
      </div>
    </div>
  )
}
