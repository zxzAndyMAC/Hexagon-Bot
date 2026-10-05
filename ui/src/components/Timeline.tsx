import { ElementReference } from './ElementReferences'
import { DesktopToolDetails } from './DesktopToolDetails'
import { memo, useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Virtuoso } from 'react-virtuoso'
import { useTimelineWindow } from '../useTimelineWindow'
import { useTimelineNodes } from '../useTimelineNodes'
import { useTimelineViewport } from '../useTimelineViewport'
import { deriveWorkbenchStatusFromFacts, factOpenTurn } from '../timelineFacts'
import { useTranslation } from 'react-i18next'
import type { TimelineItem } from '../api'
import { api, isTauri } from '../api'
import { Md, CodeBlock, ReasoningMarkdown } from './Md'
import { useUiStore } from '../store'
import { buildRows, expandedAfterPrepend, nodeMarks, NODE_ICONS, rowContainsEvent, rowKey, deriveWorkbenchStatus, stampedByAutonomy, isInterruptedTurn, openTurn, DECISION_KINDS, SYS_HIGH_RISK, type Filter, type NodeMark, type Row as ModelRow } from '../timelineModel'
import { Avatar } from './Avatar'
import { Icon } from './Icon'
import { LoadingState } from './LoadingState'
import { Row } from './Row'
import { bindingFor, formatBinding, matches } from '../keymap'
import { pairToolCalls, toolOutcome, toolFilePath, toolInputSummary, TOOL_ICON, TOOL_LABEL, EXEC_CARD_TOOLS, type ToolCall } from '../agentSteps'
import { CallStatus, ToolExecCard } from './ExecCard'
import i18n from '../i18n'
import { fmtTime } from '../usage'
import { showStickButton, TIMELINE_TAIL_PX } from '../timelineStick'
import type { TFunction } from 'i18next'

// ui-audit 票 13（P2-16）：kind/subkind 回退保留但漏 key 必须留痕——
// console.warn 一次/key，开发期漏译能被看见而不是静默吞掉。
const _warnedKeys = new Set<string>()
function trKey(t: TFunction, key: string, raw: string): string {
  if (i18n.exists(key)) return t(key)
  if (!_warnedKeys.has(key)) {
    _warnedKeys.add(key)
    console.warn(`[i18n] missing key: ${key}`)
  }
  return raw.replace(/_/g, ' ')
}

// ---- 渲染 ----

// 票 14：fmtTime 收敛到 usage.ts（Intl 本地语序）。

const chipCls = (s: string) =>
  s === 'passed' ? 'ok' : s === 'queued' ? 'warn' : s === 'rejected' || s === 'failed' ? 'err' : 'err'

// Thinking 升级（beautiful-ui 票 03）：live 态 shimmer 走秒，
// live→settled 翻转成「思考用时 Ns」；历史消息无计时源，回退原标签。
// 「思考」只承载推理文本（CONTEXT.md 定名）——工具调用归 ToolChips，不混排。
export function ThinkingRow({ text, live, plan = false }: { text: string; live?: boolean; plan?: boolean }) {
  const { t } = useTranslation()
  // 2026-10-01 用户验收：生成时可读，结束默认收起；手动选择不被增量覆盖。
  const [choice, setChoice] = useState<boolean | null>(null)
  const open = choice ?? live === true
  const bodyRef = useRef<HTMLDivElement>(null)
  const followTail = useRef(true)
  const bodyId = useId()
  const [now, setNow] = useState(0)
  const [start, setStart] = useState<number | null>(null)
  const [end, setEnd] = useState<number | null>(null)
  const trimmed = text.trim()
  const hasText = trimmed !== ''
  const timed = live !== undefined
  // 渲染期派生态调整（React 认可模式，NodeRail 同款）：
  // 首个非空增量起表，live→false 冻结，reset 清空归零重起；
  // live 未传（历史消息）= 无计时源不起表，不回填假时长。
  if (timed) {
    if (hasText && start == null) setStart((s) => s ?? Date.now())
    else if (!hasText && start != null) { setStart(null); setEnd(null) }
    else if (live === false && start != null && end == null) setEnd(() => Date.now())
  }
  useEffect(() => {
    if (!live) return
    const iv = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(iv)
  }, [live])
  // 2026-10-01: follow committed Markdown, not every incoming token. Reading
  // scrollHeight before deferred content commits forced layout and missed its tail.
  const followRenderedTail = useCallback(() => {
    const body = bodyRef.current
    if (body && live && open && followTail.current) body.scrollTop = body.scrollHeight
  }, [live, open])
  if (!hasText) return null
  const endAt = end ?? (now > 0 ? now : null)
  const secs = start != null && endAt != null ? Math.max(0, (endAt - start) / 1000) : null
  const label = plan ? t(live ? 'timeline.planning' : 'timeline.plan') : live
    ? `${t('timeline.thinking')} · ${Math.floor(secs ?? 0)}s`
    : timed && secs != null
      ? t('timeline.thought', { s: secs < 1 ? '<1' : String(Math.round(secs)) })
      : t('timeline.thinking')
  return (
    <div className="thinking-row" data-thinking="" data-open={open ? '1' : '0'}>
      <button type="button" className="thinking-toggle" aria-expanded={open} aria-controls={bodyId}
        onClick={() => { setChoice(!open); followTail.current = true }}>
        <Icon name={open ? 'chevron-down' : 'chevron-right'} size={12} />
        <span className={live ? 'shimmer-text' : undefined}>{label}</span>
      </button>
      {open && <div id={bodyId} ref={bodyRef} className="thinking-full" tabIndex={0}
        role="region" aria-label={t(plan ? 'timeline.plan' : 'timeline.thinking')}
        onScroll={(e) => {
          const el = e.currentTarget
          followTail.current = el.scrollHeight - el.scrollTop - el.clientHeight < 32
        }}><ReasoningMarkdown text={trimmed} live={live} onRender={followRenderedTail} /></div>}
    </div>
  )
}

// 断网等网角标（network-resilience 票 02）：等网中气泡转「重连中」——
// 琥珀脉点 + 等网时长计时。与「回复中」同一槽位互斥切换：等网期间
// 流式增量本来就停，两个状态叠着报只会互相撒谎。
function ReconnectChip({ since }: { since: number }) {
  const { t } = useTranslation()
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const iv = setInterval(() => setNow(Date.now()), 500)
    return () => clearInterval(iv)
  }, [])
  const s = Math.max(0, now - since) / 1000
  const elapsed = s < 60 ? `${s.toFixed(0)}s` : `${Math.floor(s / 60)}m ${(s % 60).toFixed(0)}s`
  return (
    <span data-net-waiting="" className="stream-live net-waiting">
      <span className="stream-live-dot net-waiting-dot" />
      {t('timeline.reconnecting')} · {elapsed}
    </span>
  )
}

// 进行中的一条回复（hands-free 票 06）：增量追加在同一气泡，不另起多条。
export function LiveReply({
  role,
  text,
  thinking,
  generating,
  netWaiting,
  avatar,
}: {
  role: string
  text: string
  thinking: string
  generating: boolean
  /// 等网进入时刻（票 NR-02）：非空 = 该 agent 在等网——气泡不切走、
  /// 已收文本保留，只把状态角标换成「重连中」。
  netWaiting?: number
  avatar?: ReactNode
}) {
  const { t } = useTranslation()
  return (
    <div className="msg" data-live-reply="" style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 9 }}>
      {avatar}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, marginBottom: 3 }}>
          <span style={{ fontWeight: 560, fontSize: 12 }}>{role}</span>
          {generating && netWaiting != null && <ReconnectChip since={netWaiting} />}
          {generating && netWaiting == null && (
            <span data-generating="" className="stream-live">
              <span className="stream-live-dot" />
              {t('timeline.streaming')}
            </span>
          )}
        </div>
        {/* 思考 live = 回合在跑且正文未出（正文一到即落定计时） */}
        <ThinkingRow text={thinking} live={generating && !text} />
        {text ? <CollapsibleBody text={text} live={generating} /> : null}
      </div>
    </div>
  )
}

export function StatusLine() {
  const { t } = useTranslation()
  const stages = useUiStore((s) => s.stages)
  const team = useUiStore((s) => s.team)
  const timeline = useUiStore((s) => s.timeline)
  const timelineCaughtUp = useUiStore((s) => s.timelineCaughtUp)
  const streams = useUiStore((s) => s.streams)
  const thinkings = useUiStore((s) => s.thinkings)
  const streamDone = useUiStore((s) => s.streamDone)
  const facts = useUiStore(s => s.timelineFacts)
  const projectRoot = useUiStore(s => s.projectRoot)
  const st = facts || projectRoot ? deriveWorkbenchStatusFromFacts({ stages, team, facts, streams, thinkings, streamDone })
    : deriveWorkbenchStatus({ stages, team, timeline, streams, thinkings, streamDone, timelineCaughtUp })
  if (!st.stage && !st.role && !st.tool) return null
  return (
    <div data-status-line="" className="status-line">
      {st.stage && <span>{t('timeline.statusStage', { stage: st.stage })}</span>}
      {st.role && <span>{t('timeline.statusRole', { role: st.role })}</span>}
      {st.tool && <span>{t('timeline.statusTool', { tool: st.tool })}</span>}
    </div>
  )
}

// ui-audit 票 11（P3-23）：超高消息体渐隐夹持 + 展开/收起。
// 原实现 max-height+内滚——滚到底才看得见尾巴，扫读时不知道藏了内容。
function CollapsibleBody({ text, unclamped, live }: { text: string; unclamped?: boolean; live?: boolean }) {
  const { t } = useTranslation()
  const ref = useRef<HTMLDivElement>(null)
  const [tall, setTall] = useState(false)
  const [open, setOpen] = useState(false)
  // useLayoutEffect：测高必须抢在 paint 前——useEffect 会先画一帧未夹持全文
  // 再夹，虚拟列表滚回重挂时每条长消息都闪一下（owner 反馈滚动抖动）。
  useLayoutEffect(() => {
    // 票 18：story 档全量展开，不测高不夹持
    setTall(!unclamped && (ref.current?.scrollHeight ?? 0) > 260)
  }, [text, unclamped])
  return (
    <>
      {/* live=流式生成中（票 06）：最新块 stream-in + 尾端 caret，均由 CSS 驱动 */}
      <div ref={ref} className={`msg-body${live ? ' live' : ''}${tall && !open ? ' clamped' : ''}`}>
        <Md>{text}</Md>
      </div>
      {tall && (
        <button className="msg-toggle" onClick={() => setOpen(!open)}>
          {t(open ? 'timeline.collapse' : 'timeline.expand')}
        </button>
      )}
    </>
  )
}

function SystemRow({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  // ui-audit 票 05（P1-4）：system 事件按 payload.kind 子类定名——
  // 「系统」一刀切让 invariant_violation 这类信号淹没在同名行里。
  // 词表外子类回退原始 kind 文本（自文档化，不吞新类）。
  const sub = item.event.kind === 'system' ? String(item.event.payload?.kind ?? '') : ''
  if (sub === 'agent_plan' && typeof item.event.payload.text === 'string') {
    return <div className="plan-row"><ThinkingRow text={item.event.payload.text} plan /></div>
  }
  const stopped = isInterruptedTurn(item)
  const highRisk = (item.event.kind === 'turn_failed' && !stopped) || SYS_HIGH_RISK.has(sub)
  const autoStamp = item.event.kind === 'stamped' && stampedByAutonomy(item.event.payload)
  const label = stopped ? t('ev.turn_interrupted') : autoStamp
    ? t('ev.stamped_auto', { stage: String(item.event.payload?.stage ?? '') })
    : sub
      ? trKey(t, `sys.${sub}`, sub)
      : trKey(t, `ev.${item.event.kind}`, item.event.kind)
  const note = item.event.kind === 'stamp_rejected' ? String(item.event.payload?.note ?? '')
    : item.event.kind === 'turn_failed' && !stopped ? String(item.event.payload?.error ?? item.event.payload?.outcome ?? item.event.payload?.code ?? '') : ''
  return (
    <div className="sysrow">
      <div className="sysline" />
      <span className="syslabel" style={highRisk ? { color: 'var(--err)' } : undefined}>
        {label}
        {note && <span className="dim3"> · {note}</span>}
      </span>
    </div>
  )
}

const StageHeader = memo(function StageHeader({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const artifacts = useUiStore((s) => s.artifacts)
  const p = item.event.payload
  const runId = String(p.run_id ?? '')
  const n = runId ? artifacts.filter((a) => a.stage_run_id === runId).length : 0
  return (
    <div style={{ padding: '18px 14px 6px', display: 'flex', alignItems: 'center', gap: 10 }}>
      <span style={{ color: 'var(--accent)', display: 'inline-flex' }}><Icon name="stamp" size={11} /></span>
      <span style={{ fontWeight: 560, fontSize: 13 }}>{String(p.stage ?? '')}</span>
      {n > 0 && <span className="chip ok">{n} {t('side.artifacts').toLowerCase()}</span>}
      <div className="sysline" style={{ flex: 1 }} />
    </div>
  )
})

function ReturnSummaryRow({ item, onJumpEvent }: { item: TimelineItem; onJumpEvent?: (eventId: number) => void }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const p = item.event.payload as Record<string, unknown>
  // 票 16（方向卡 1）：since_event = 离开时刻锚点，行点击跳回该位置。
  const sinceEvent = Number(p.since_event ?? 0)
  // 票 13（P2-16）：载荷形状 = autonomy.rs ReturnSummary——
  // reviews/flags/permissions/stages 计数组 + deliveries[{path,kind}]
  // + pending_todos[{kind,count}]（数组！旧代码当数字读 → NaN 角标）。
  // 组/字段名走 rs.group.*/rs.key.*；未知字段回退原值 + warn（不吞新字段）。
  const lines: string[] = []
  for (const g of ['reviews', 'flags', 'permissions', 'stages'] as const) {
    const m = p[g] as Record<string, unknown> | undefined
    if (!m) continue
    for (const [k, v] of Object.entries(m)) {
      if (Number(v) > 0) lines.push(`${trKey(t, `rs.group.${g}`, g)} · ${trKey(t, `rs.key.${k}`, k)}: ${v}`)
    }
  }
  const deliveries = Array.isArray(p.deliveries) ? (p.deliveries as { path?: unknown }[]) : []
  for (const d of deliveries) {
    if (d?.path) lines.push(`${trKey(t, 'rs.group.deliveries', 'deliveries')} · ${String(d.path)}`)
  }
  const todos = Array.isArray(p.pending_todos) ? (p.pending_todos as { kind?: unknown; count?: unknown }[]) : []
  const attention = p.attention as Record<string, unknown> | undefined
  const attentionKeys = ['unresolved_actions', 'unknown_cost_records', 'budget_stops', 'exception_decisions', 'policy_candidates'] as const
  const pendingN = todos.reduce((s, x) => s + Number(x?.count ?? 0), 0)
  for (const x of todos) {
    if (Number(x?.count) > 0) lines.push(`${trKey(t, 'rs.group.todos', 'todos')} · ${String(x.kind)}: ${x.count}`)
  }
  return (
    <Row className="card-ask" style={{ margin: '6px 14px', padding: '8px 14px', cursor: 'pointer' }} onClick={() => setOpen(!open)}>
      <div style={{ fontWeight: 510, color: 'var(--accent)', fontSize: 12, display: 'flex', alignItems: 'center', gap: 5 }}>
        <Icon name="list" size={11} /> {t('cards.returnSummary')}
        {deliveries.length > 0 && <span className="chip ok">{deliveries.length} {t('side.artifacts').toLowerCase()}</span>}
        <Icon name={open ? 'chevron-down' : 'chevron-right'} size={9} />
        {pendingN > 0 && <span className="chip amber" style={{ marginLeft: 4 }}>{t('cards.pending', { count: pendingN })}</span>}
      </div>
      {attention && <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5, marginTop: 6 }}>
        {attentionKeys.filter((key) => Number(attention[key]) > 0).map((key) => <span
          key={key} className={`chip ${key === 'unresolved_actions' ? 'err' : 'warn'}`}>
          {t(`returnAttention.${key}`, { count: Number(attention[key]) })}
        </span>)}
      </div>}
      {open && (
        <div className="mono dim" style={{ fontSize: 11, marginTop: 6 }}>
          {lines.length
            ? lines.map((ln, i) => (
                <Row
                  key={i}
                  role="button"
                  title={t('rs.jumpToMark')}
                  onClick={(e) => { e.stopPropagation(); if (sinceEvent > 0) onJumpEvent?.(sinceEvent) }}
                  style={{ cursor: sinceEvent > 0 ? 'pointer' : 'default', padding: '1px 4px', borderRadius: 3, whiteSpace: 'pre-wrap' }}
                >
                  {ln}
                </Row>
              ))
            : '—'}
        </div>
      )}
    </Row>
  )
}

// memo：滚动中贴底翻转/flash 等父级重渲不再连带可见行整棵重渲
//（remark 重解析是最贵的一帧）；store 订阅（team/openTab）不受 memo 挡。
export const EventRow = memo(function EventRow({
  item,
  steered,
  turnBoundary,
  turnActive,
  onJumpEvent,
  story,
}: {
  item: TimelineItem
  steered?: Set<number>
  turnBoundary?: number
  turnActive?: boolean
  onJumpEvent?: (eventId: number) => void
  story?: boolean
}) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const openTab = useUiStore((s) => s.openTab)
  const k = item.event.kind
  // 票 08：封闭选择的结果单独成行，不折进系统事件组——负责人要一眼看到派给了谁。
  if (k === 'pm_routed') {
    const held = item.event.payload.held === true
    const rejected = item.event.payload.rejected === true
    const role = String(item.event.payload.role ?? '')
    const instance = item.event.payload.scope === 'role_instances' ? item.event.payload.agent_id : null
    const target = instance ? `${role} · ${String(instance)}` : role
    const label = rejected
      ? t('timeline.routeRejected')
      : held
        ? t('timeline.routeHold')
        : t('timeline.routedTo', { role: target })
    return (
      <div className="sysrow" data-route={rejected ? 'rejected' : held ? 'hold' : role}>
        <div className="sysline" />
        <span className="syslabel">{label}</span>
      </div>
    )
  }
  if (k === 'stage_started') return <StageHeader item={item} />
  if (k === 'return_summary') return <ReturnSummaryRow item={item} onJumpEvent={onJumpEvent} />
  if (k === 'artifact_delivered') {
    const status = String(item.event.payload.status ?? '')
    const path = String(item.event.payload.path ?? '')
    return (
      <div
        className="sysrow"
        style={{ cursor: 'pointer' }}
        title={path}
        onClick={() => path && openTab({ id: `art:${path}`, kind: 'artifact', title: path, path })}
      >
        <div className="sysline" />
        <span className="syslabel" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <span className="mono" style={{ color: 'var(--accent)', textDecoration: 'underline', textUnderlineOffset: 3 }}>
            {path}
          </span>
          <span className={`chip ${chipCls(status)}`}>{status}</span>
        </span>
      </div>
    )
  }
  if (k === 'flag_submitted') {
    const ev = item.event
    const severity = String(ev.payload.severity ?? '')
    return (
      <div className="card card-flag" style={{ margin: '4px 14px', padding: '8px 12px' }}>
        <div style={{ fontSize: 12 }}>
          <span className={`chip ${chipCls(severity)}`} style={{ marginRight: 8 }}>{severity}</span>
          {String(ev.payload.target ?? '')}
          {ev.payload.section ? ` · ${String(ev.payload.section)}` : ''}
        </div>
        <div className="dim" style={{ marginTop: 4, fontSize: 12 }}>{String(ev.payload.reason ?? '')}</div>
      </div>
    )
  }
  if (k === 'flag_adjudicated') {
    const ev = item.event
    const agree = Boolean(ev.payload.agree)
    return (
      <div className="sysrow">
        <div className="sysline" />
        <span className="syslabel" style={{ display: 'inline-flex', gap: 6, alignItems: 'center' }}>
          <Icon name="flag" size={9} />
          <span className={`chip ${agree ? 'ok' : 'err'}`}>
            {agree ? t('cards.agreeContinue') : t('cards.rejectContinue')}
          </span>
          <span className="mono">{String(ev.payload.flag_id ?? '')}</span>
          <span className="dim3">· {String(ev.payload.by ?? '')}</span>
        </span>
      </div>
    )
  }
  if (item.message) {
    const m = item.message
    const isOwner = item.event.kind === 'owner_message' || m.author === 'owner'
    const member = team.find((x) => x.id === m.author)
    const title = isOwner ? t('timeline.owner') : (member?.role ?? m.author)
    const time = fmtTime(item.event.created_at)
    if (isOwner) {
      return (
        <div className="msg" style={{ flexDirection: 'row', justifyContent: 'flex-end', alignItems: 'flex-start', gap: 9 }}>
          <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column', alignItems: 'flex-end' }}>
            <div className="msg-author dim" style={{ textAlign: 'right' }}>
              {title}{time && <span className="dim3" style={{ marginLeft: 6 }}>{time}</span>}
              {/* 票 05 steering 状态：steering_injected 事件到 → 已送达；
                  回合进行中未注入 → 下回合生效（可能提前——下一检查点即排水） */}
              {steered?.has(m.id) && (
                <span className="chip ok" style={{ marginLeft: 6, fontSize: 10 }}>{t('timeline.steeringDelivered')}</span>
              )}
              {!steered?.has(m.id) && turnActive && item.event.id > (turnBoundary ?? -1) && (
                <span className="chip warn" style={{ marginLeft: 6, fontSize: 10 }}>{t('timeline.steeringQueued')}</span>
              )}
            </div>
            <CollapsibleBody text={m.body} unclamped={story} />
            {m.element_refs?.map(reference => <ElementReference key={reference.id} reference={reference} />)}
            {/* 票 03：图片附件引用 chip（字节在 .hexagon/inbox/，
                不开 assetProtocol——渲染引用不渲染图本体） */}
            {m.attachments?.length > 0 && (
              <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 4, justifyContent: 'flex-end' }}>
                {m.attachments.map((a) => (
                  <span key={a.path} className="chip" style={{ fontSize: 10, display: 'inline-flex', alignItems: 'center', gap: 4 }} title={a.path}>
                    <Icon name="artifact" size={10} /> {a.name}
                  </span>
                ))}
              </div>
            )}
          </div>
          <Avatar agentId="owner" role={title} size={34} />
        </div>
      )
    }
    return (
      <div className="msg" style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 9 }}>
        {member && (
          <span
            style={{ cursor: 'pointer', flexShrink: 0 }}
            title={t('agent.openTab')}
            onClick={() => openTab({ id: `agent:${member.id}`, kind: 'agent', title: member.role, agentId: member.id, role: member.role })}
          >
            <Avatar agentId={m.author} role={member.role} size={34} />
          </span>
        )}
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, marginBottom: 3 }}>
            <span style={{ fontWeight: 560, fontSize: 12 }}>{title}</span>
            {time && <span className="dim3" style={{ fontSize: 10 }}>{time}</span>}
          </div>
          <ThinkingRow text={m.thinking} />
          <CollapsibleBody text={m.body} unclamped={story} />
        </div>
      </div>
    )
  }
  return <SystemRow item={item} />
})

// ToolChips 移植（beautiful-ui 票 02，借形 MIT slev12397/beautiful-ui）：
// 折叠组展开成逐行 chip——图标+定名+mono 参数片，行点开看 input/result 明细，
// 组尾文件片点开产物 tab。原作的悬停 diff 预览未移植：core 把 fs_write 的
// content scrub 成 bytes（safety.rs），事件里没有增删行数据，不做假预览。
function ToolChipRow({ call, delay, animate = true }: { call: ToolCall; delay: number; animate?: boolean }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const p = call.called.event.payload as Record<string, unknown>
  const tool = String(p.tool ?? '')
  const res = call.result?.event.payload as Record<string, unknown> | undefined
  const ok = toolOutcome(call.result)
  const envelope = res?.result as { output?: Record<string, unknown> & { source?: string; status?: string } } | undefined
  const loadedSkill = tool === 'load_skill' && ok === true && envelope?.output?.status === 'loaded' ? envelope.output : undefined
  const summary = toolInputSummary(p)
  const detail = JSON.stringify({ input: p.input ?? p, ...(res ? { result: res } : {}) }, null, 2)
  return (
    <div style={animate ? { animation: `fade-up 300ms cubic-bezier(0.23,1,0.32,1) ${delay}ms both` } : undefined}>
      <button
        type="button"
        className="tchip-row"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="dim3" style={{ display: 'inline-flex' }}>
          <Icon name={TOOL_ICON[tool] ?? 'tool'} size={10} />
        </span>
        <span className="tchip-label">
          {t(`agent.${TOOL_LABEL[tool] ?? 'stepTool'}`, { defaultValue: tool })}
        </span>
        {summary && <span className="tool-chip">{summary}</span>}
        {/* 票 05 TaskRows 状态机：无 result=在途运行环（--warn 蓝槽）；
            落定翻 check/X 徽标 pop-in。琥珀是「在等你」专色不给在途。
            exec-cards 票 02 起与 ExecCard 共用 CallStatus。 */}
        <CallStatus ok={ok} />
        <span className="dim3" style={{ display: 'inline-flex' }}>
          <Icon name={open ? 'chevron-down' : 'chevron-right'} size={9} />
        </span>
      </button>
      {open && (
        <div className="tchip-detail">
          {(tool.startsWith('computer_') || tool.startsWith('browser_')) && envelope?.output && <DesktopToolDetails key={call.called.event.project_id} projectId={call.called.event.project_id} capturedAt={call.result?.event.created_at} output={envelope.output} />}
          {loadedSkill && <p>{t('agent.skillLoaded')} · {t('agent.skillSource')}: <code>{loadedSkill.source}</code></p>}
          <CodeBlock code={detail} lang="json" />
        </div>
      )}
    </div>
  )
}

export const ToolGroupRow = memo(function ToolGroupRow({ items, callIndex, expanded, idx, onToggle, fresh }: {
  items: TimelineItem[]
  callIndex?: ReadonlyMap<number, ToolCall>
  expanded: boolean
  idx: number
  onToggle: (idx: number) => void
  // fresh=true：本次展开动作后的首挂——入场动画只播这一回；
  // 之后滚动重挂载 fresh=false，不再重放（虚拟列表行会被卸载重建）。
  fresh?: boolean
}) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const openTab = useUiStore((s) => s.openTab)
  const agentId = items[0]?.event.agent_id
  const member = agentId ? team.find((x) => x.id === agentId) : undefined
  // 计数按调用对（called 吸收 result），不按事件条数——旧版 2N 的数会翻倍。
  const calls = useMemo(() => pairToolCalls(items).map((call) => callIndex?.get(call.called.event.id) ?? call), [items, callIndex])
  const files = useMemo(() => {
    const seen = new Set<string>()
    for (const c of calls) {
      const path = toolFilePath(c.called.event.payload as Record<string, unknown>)
      if (path) seen.add(path)
    }
    return [...seen]
  }, [calls])
  if (!calls.length) return null
  return (
    <div>
      <div
        className="sysrow"
        style={{ cursor: 'pointer', userSelect: 'none' }}
        onClick={() => onToggle(idx)}
      >
        <div className="sysline" />
        <span className="syslabel dim" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}>
          {member && <Avatar agentId={member.id} role={member.role} size={14} />}
          <Icon name="tool" size={10} /> {t('timeline.toolCalls', { count: calls.length })} <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={9} />
        </span>
      </div>
      {expanded && (
        <div style={{ paddingLeft: 24, paddingRight: 14 }}>
          {/* exec-cards 票 02（spec D2）：重负载升 ExecCard，轻量读系保持 chip 行 */}
          {calls.map((c, i) => {
            const tool = String((c.called.event.payload as Record<string, unknown>).tool ?? '')
            return <div key={c.called.event.id} data-timeline-event={c.called.event.id} data-timeline-result={c.result?.event.id}>
              {EXEC_CARD_TOOLS.has(tool)
                ? <ToolExecCard call={c} delay={Math.min(i, 12) * 45} animate={fresh} />
                : <ToolChipRow call={c} delay={Math.min(i, 12) * 45} animate={fresh} />}
            </div>
          })}
          {files.length > 0 && (
            <div className="tchip-files">
              {files.map((path, i) => (
                <button
                  key={path}
                  type="button"
                  className="chip chip-btn mono"
                  style={fresh
                    ? { fontSize: 10, animation: `pop-in 250ms cubic-bezier(0.23,1,0.32,1) ${Math.min(i, 10) * 60}ms both` }
                    : { fontSize: 10 }}
                  title={path}
                  onClick={() => openTab({ id: `art:${path}`, kind: 'artifact', title: path, path })}
                >
                  <Icon name="file" size={9} /> {path}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
})

export const SysGroupRow = memo(function SysGroupRow({ items, expanded, idx, onToggle }: {
  items: TimelineItem[]
  expanded: boolean
  idx: number
  onToggle: (idx: number) => void
}) {
  const { t } = useTranslation()
  return (
    <div>
      <div
        className="sysrow"
        style={{ cursor: 'pointer', userSelect: 'none' }}
        onClick={() => onToggle(idx)}
      >
        <div className="sysline" />
        <span className="syslabel dim" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}>
          <Icon name="list" size={10} /> {t('timeline.sysEvents', { count: items.length })} <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={9} />
        </span>
      </div>
      {expanded && items.map((it) => <div key={it.event.id} data-timeline-event={it.event.id}><SystemRow item={it} /></div>)}
    </div>
  )
})

// exec-cards 票 03：回合摘要行（Cursor Worked-for-Xs 借形）——已收束回合的
// 执行行收成单行（角色 · 工具数 · 耗时），点开就地展开还原被折行。
// 中性色：摘要是收纳不是信号；failed 回合挂 err 徽标（唯一需扫读区分的状态）。
const TurnSummaryRow = memo(function TurnSummaryRow({
  row,
  expanded,
  idx,
  onToggle,
  children,
}: {
  row: Extract<ModelRow, { type: 'turnsummary' }>
  expanded: boolean
  idx: number
  onToggle: (idx: number) => void
  children?: ReactNode
}) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const member = row.agentId ? team.find((x) => x.id === row.agentId) : undefined
  const role = member?.role ?? row.agentId ?? ''
  const secs = row.secs == null ? '?' : row.secs < 1 ? '<1' : String(Math.round(row.secs))
  return (
    <div>
      <div
        className="sysrow turn-summary"
        style={{ cursor: 'pointer', userSelect: 'none' }}
        role="button"
        tabIndex={0}
        aria-expanded={expanded}
        onClick={() => onToggle(idx)}
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onToggle(idx) } }}
      >
        <div className="sysline" />
        <span className="syslabel dim" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}>
          {member && <Avatar agentId={member.id} role={member.role} size={14} />}
          <Icon name="bolt" size={10} />
          {t('timeline.turnSummary', { role, count: row.calls, secs })}
          {row.failed && <span className="chip err">{t('timeline.turnFailed')}</span>}
          <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={9} />
        </span>
      </div>
      {expanded && <>{children}{row.partial && <div className="dim3" style={{ padding: '4px 14px 8px', fontSize: 11 }}>{t('timeline.partialTurn')}</div>}</>}
    </div>
  )
})

// ---- 语义节点轨（hover 展开 / 点击跳转 / 待决脉动）----

function NodeRail({ marks, onJump }: { marks: NodeMark[]; onJump: (m: NodeMark) => void }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const listRef = useRef<HTMLDivElement>(null)
  // ui-audit 票 12（P2-13）：mod+J 键盘入口——pulse 递增即展开轨道
  // 并聚焦首条刻度（Row 原语：Tab 可达、Enter/Space 激活）。
  // 渲染期比较（React 派生态模式）而非 effect-setState，避免 lint 噪声。
  const pulse = useUiStore((s) => s.nodeRailPulse)
  const [seenPulse, setSeenPulse] = useState(0)
  if (pulse !== seenPulse) {
    setSeenPulse(pulse)
    if (pulse > 0) setOpen(true)
  }
  useEffect(() => {
    if (!pulse) return
    requestAnimationFrame(() => {
      listRef.current?.querySelector<HTMLElement>('[tabindex]')?.focus()
    })
  }, [pulse])
  useEffect(() => {
    if (!open) return
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false) }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [open])
  return (
    <div
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      style={{
        position: 'absolute', right: 0, top: 0, bottom: 0, width: open ? 180 : 14,
        zIndex: 20, display: 'flex', transition: 'width .15s',
        background: open ? 'var(--popover)' : 'transparent', // 浮层规范（workbench-polish 01）：抽屉叠在事件流上，必须不透明
        borderLeft: open ? '1px solid var(--border)' : 'none',
      }}
    >
      {!open ? (
        <div
          title={`${t('rail.nodes')} ${formatBinding(bindingFor('nodeRail'))}`}
          style={{ width: 14, height: '100%', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6, paddingTop: 40, overflow: 'hidden' }}
        >
          {marks.map((m, i) => (
            // ADR 0056-3：待决=琥珀 accent（需人注意专色）；
            // --warn 蓝留给 steering_queued 这类信息性排队态。
            <span key={i} style={{ color: m.pending ? 'var(--accent)' : 'var(--text-3)', display: 'inline-flex' }}>
              <Icon name={m.icon} size={8} />
            </span>
          ))}
        </div>
      ) : (
        <div ref={listRef} role="listbox" style={{ flex: 1, overflowY: 'auto', padding: '8px 8px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
            <span className="dim3" style={{ fontSize: 10, fontWeight: 560 }}>{t('rail.nodes')}</span>
            <button className="icon-btn" style={{ display: 'inline-flex', alignItems: 'center' }} onClick={() => setOpen(false)}><Icon name="close" size={9} /></button>
          </div>
          {marks.map((m, i) => (
            <Row
              key={i}
              role="option"
              onClick={() => { onJump(m); setOpen(false) }}
              style={{
                display: 'flex', gap: 6, alignItems: 'center', padding: '3px 4px', borderRadius: 4,
                cursor: 'pointer', fontSize: 11,
                color: m.pending ? 'var(--accent)' : 'var(--text)',
                animation: m.pending ? 'pulse-amber 1.6s infinite' : undefined,
              }}
            >
              <Icon name={m.icon} size={11} />
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.label}</span>
            </Row>
          ))}
        </div>
      )}
    </div>
  )
}

// ---- 流式气泡（票 03 + ui-audit 票 08 交接）：回合进行中的瞬时增量挂在
// 时间线尾部；done 到达不删气泡——原地转落位样式（去掉 streaming 标签，
// 与持久消息行同构同宽），等 refreshFast 拉到同 agent 持久消息才清除。
// 交接语义取「持久化确认后」而非「done 即清」：后者留最长 2s 内容空窗
// + 排版跳变（P1-7 实测），前者气泡与落库行无缝替换。
function joinCalls(buf: Record<number, string> | undefined): string {
  if (!buf) return ''
  return Object.keys(buf)
    .map(Number)
    .sort((a, b) => a - b)
    .map((c) => buf[c])
    .filter((s) => s.length > 0)
    .join('\n\n')
}

// 首 token 等待位（beautiful-ui 票 04）：回合已开、流缓冲未到的死寂段——
// 像素格占住气泡位，告诉负责人 agent 在跑而非卡死。
// 缓冲一到即被 LiveReply 顶掉（ids 判据同一处），无交接空窗。
export function WaitingReply({ role, startedAt, netWaiting, avatar }: {
  role: string
  startedAt?: number | null
  /// 等网进入时刻（票 NR-02）：首 token 未到就断网时，占位气泡的
  /// 标签从「执行中」转「重连中」，计时起点 = 等网起点（不是回合起点）。
  netWaiting?: number
  avatar?: ReactNode
}) {
  const { t } = useTranslation()
  return (
    <div className="msg" data-waiting="" style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 9 }}>
      {avatar}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, marginBottom: 3 }}>
          <span style={{ fontWeight: 560, fontSize: 12 }}>{role}</span>
        </div>
        <LoadingState
          label={netWaiting != null ? t('timeline.reconnecting') : t('timeline.working')}
          startedAt={netWaiting ?? startedAt ?? undefined}
        />
      </div>
    </div>
  )
}

function StreamFooter() {
  // 窄订阅：整店订阅会让任何无关 store 更新都重渲 Footer（流式期间每 delta 一次）
  const plans = useUiStore((s) => s.plans)
  const streams = useUiStore((s) => s.streams)
  const thinkings = useUiStore((s) => s.thinkings)
  const streamDone = useUiStore((s) => s.streamDone)
  const waitingSince = useUiStore((s) => s.waitingSince)
  const team = useUiStore((s) => s.team)
  const timeline = useUiStore((s) => s.timeline)
  const timelineCaughtUp = useUiStore((s) => s.timelineCaughtUp)
  const ids = [...new Set([...Object.keys(streams), ...Object.keys(thinkings), ...Object.keys(plans)])].filter((id) =>
    !!plans[id] || Object.values(streams[id] ?? {}).some((s) => s.length > 0) ||
    Object.values(thinkings[id] ?? {}).some((s) => s.length > 0),
  )
  const facts = useUiStore(s => s.timelineFacts)
  const projectRoot = useUiStore(s => s.projectRoot)
  const turn = facts || projectRoot ? factOpenTurn(facts) : openTurn(timeline, timelineCaughtUp)
  const waitingId = turn.agentId && !ids.includes(turn.agentId) ? turn.agentId : null
  const waitingMember = waitingId ? team.find((x) => x.id === waitingId) : undefined
  // 底部留白带恒在内容总高里。贴底写的是 scrollHeight，不是最后一行的底边。
  const tail = <div data-timeline-tail="" style={{ height: TIMELINE_TAIL_PX }} />
  if (!ids.length && !waitingId) return tail
  return (
    <div>
      {waitingId && (
        <WaitingReply
          role={waitingMember?.role ?? waitingId}
          startedAt={turn.at}
          netWaiting={waitingSince[waitingId]}
          avatar={waitingMember ? <Avatar agentId={waitingId} role={waitingMember.role} size={34} /> : undefined}
        />
      )}
      {ids.map((id) => {
        const member = team.find((x) => x.id === id)
        const done = streamDone[id] != null
        return (
          <div key={id}>
          {plans[id] && <div className="plan-row"><ThinkingRow plan text={plans[id]}
            live={!done && !Object.keys(streams[id] ?? {}).some((call) => Number(call) > 0)} /></div>}
          <LiveReply
            role={member?.role ?? id}
            text={joinCalls(streams[id])}
            thinking={joinCalls(thinkings[id])}
            generating={!done}
            netWaiting={waitingSince[id]}
            avatar={member ? <Avatar agentId={id} role={member.role} size={34} /> : undefined}
          />
          </div>
        )
      })}
      {tail}
    </div>
  )
}

function TimelineTail() { return <div data-timeline-tail="" style={{ height: TIMELINE_TAIL_PX }} /> }

const EMPTY_TIMELINE: TimelineItem[] = []

// ---- 主体 ----

export function Timeline() {
  const { t } = useTranslation()
  const { timeline: legacyTimeline, pending, streams, team, projectRoot, projectEpoch, timelineFacts } = useUiStore()
  const [filter, setFilter] = useState<Filter>('all')
  const windowState = useTimelineWindow(filter)
  const { page } = windowState
  const { nodes } = useTimelineNodes()
  const timeline = page?.items ?? (projectRoot == null && projectEpoch === 0 ? legacyTimeline : EMPTY_TIMELINE)
  const callIndex = useMemo(() => new Map(pairToolCalls([...timeline, ...(page?.boundary_pairs ?? [])]
    .sort((a, b) => a.event.id - b.event.id)).map(call => [call.called.event.id, call])), [timeline, page?.boundary_pairs])
  const stickReq = useUiStore(s => s.timelineStickReq)
  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [flash, setFlash] = useState<string | null>(null)
  const [exported, setExported] = useState<number | null>(null)
  const [jumpTarget, setJumpTarget] = useState<number | null>(null)
  const expandAt = useRef(new Map<number, number>())
  const ANIM_MS = 1000
  const rows = useMemo(() => buildRows(timeline, filter, projectRoot ? page ?? undefined : undefined), [timeline, filter, page, projectRoot])
  const [previousRows, setPreviousRows] = useState({ rows, epoch: projectEpoch })
  if (previousRows.rows !== rows || previousRows.epoch !== projectEpoch) {
    setPreviousRows({ rows, epoch: projectEpoch })
    if (previousRows.epoch !== projectEpoch) setExpanded(new Set())
    else if (windowState.change === 'prepend') setExpanded(expandedAfterPrepend(previousRows.rows, rows, expanded))
  }
  const viewport = useTimelineViewport({ rows, generation: windowState.generation, change: windowState.change,
    loading: windowState.loading || !!windowState.error || (projectEpoch > 0 && projectRoot == null), enabled: projectRoot != null || projectEpoch > 0,
    hasBefore: page?.has_before ?? false, hasAfter: page?.has_after ?? false,
    olderLoading: windowState.olderLoading, olderError: windowState.olderError,
    loadOlder: windowState.loadOlder, loadLatest: windowState.loadLatest,
    targetEventId: page?.target_event_id ?? null, stickReq })
  const marks = useMemo(() => projectRoot != null ? [
    ...(pending.length ? [{ rowIdx: -1, icon: 'warn' as const, label: String(pending.length), pending: true }] : []),
    ...nodes.map(node => ({ eventId: node.event_id, rowIdx: rows.findIndex(row => rowContainsEvent(row, node.event_id)),
      icon: NODE_ICONS[node.kind] ?? 'list', label: node.label })),
  ] : nodeMarks(timeline, rows, pending.length), [projectRoot, nodes, rows, pending.length, timeline])
  const { steered, turnBoundary } = useMemo(() => {
    const injected = new Set<number>(page?.steered_message_ids ?? [])
    let boundary = timelineFacts?.latest_turn_start_id ?? -1
    for (const it of timeline) {
      if (!timelineFacts && it.event.kind === 'turn_started') boundary = it.event.id
      if (it.event.kind === 'system' && it.event.payload?.kind === 'steering_injected') {
        const mid = Number(it.event.payload.msg_id)
        if (mid) injected.add(mid)
      }
    }
    return { steered: injected, turnBoundary: boundary }
  }, [timeline, page?.steered_message_ids, timelineFacts])
  const turnActive = Object.keys(streams).length > 0
  const watermark = timelineFacts?.latest_event_id ?? timeline.at(-1)?.event.id ?? 0
  const [seenWatermark, setSeenWatermark] = useState(watermark)
  useEffect(() => { if (viewport.pinned) setSeenWatermark(watermark) }, [viewport.pinned, watermark])
  useEffect(() => { setSeenWatermark(watermark) }, [windowState.generation])
  const unseen = Math.max(0, watermark - seenWatermark)
  useEffect(() => { viewport.writeTail() }, [streams, viewport.writeTail])
  useEffect(() => {
    if (jumpTarget == null || filter !== 'all') return
    void windowState.jumpAround(jumpTarget)
    setJumpTarget(null)
  }, [filter, jumpTarget, windowState.jumpAround])

  const revealRow = useCallback((row: ModelRow) => {
    setExpanded(previous => {
      const next = new Set(previous)
      if (row.type === 'toolgroup' || row.type === 'sysgroup') next.add(row.idx)
      if (row.type === 'turnsummary') { next.add(row.idx); for (const child of row.folded) next.add(child.idx) }
      return next
    })
  }, [])
  useEffect(() => {
    const target = page?.target_event_id
    if (target == null) return
    const row = rows.find(row => rowContainsEvent(row, target))
    if (row) { revealRow(row); setFlash(rowKey(row)) }
  }, [page?.target_event_id, rows, revealRow])
  useEffect(() => {
    if (flash == null) return
    const timer = setTimeout(() => setFlash(null), 1400)
    return () => clearTimeout(timer)
  }, [flash])
  const jumpToEvent = useCallback((id: number) => {
    const index = rows.findIndex(row => rowContainsEvent(row, id))
    if (index < 0) { viewport.releasePin(); setFilter('all'); setJumpTarget(id); return }
    revealRow(rows[index])
    viewport.jumpToRow(index, id)
    setFlash(rowKey(rows[index]))
  }, [rows, revealRow, viewport.releasePin, viewport.jumpToRow])
  const jump = useCallback((mark: NodeMark) => {
    if (mark.eventId != null) jumpToEvent(mark.eventId)
    else if (mark.rowIdx >= 0) jumpToEvent(rows[mark.rowIdx].idx)
    else viewport.jumpToRow(0)
  }, [jumpToEvent, rows, viewport.jumpToRow])
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (useUiStore.getState().modalScope !== 'workbench') return
      if (matches(event, bindingFor('timelineLatest'))) { event.preventDefault(); viewport.holdPin() }
      else if (matches(event, bindingFor('timelineLoadOlder'))) { event.preventDefault(); viewport.requestOlder() }
      else if (matches(event, bindingFor('timelineRetry'))) { event.preventDefault(); void windowState.retry() }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [viewport.holdPin, viewport.requestOlder, windowState.retry])
  const toggleRow = useCallback((idx: number) => {
    setExpanded((s) => {
      const n = new Set(s)
      if (n.has(idx)) {
        n.delete(idx)
        expandAt.current.delete(idx)
      } else {
        n.add(idx)
        expandAt.current.set(idx, performance.now())
      }
      return n
    })
  }, [])

  // 「刚展开」窗口内的行播入场动画；窗口外（含滚动重挂载）静止直出。
  const isFreshExpand = (idx: number) =>
    performance.now() - (expandAt.current.get(idx) ?? -1e9) < ANIM_MS

  // exec-cards 票 03：行分发提成可递归闭包——摘要行展开时被折行
  // （toolgroup/sysgroup）走同一套渲染与同一个 expanded 集合（键=行 idx 即事件 id）。
  const rowContent = (row: ModelRow): ReactNode => {
    if (row.type === 'chapter') {
      const role = team.find((m) => m.id === row.agentId)?.role ?? row.agentId ?? ''
      return (
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '14px 14px 4px' }}>
          <div className="sysline" style={{ flex: 1 }} />
          <span className="dim3" style={{ fontSize: 10, fontWeight: 560, whiteSpace: 'nowrap' }}>
            {t('timeline.chapter', { n: row.n, agent: role })}{row.stage ? ` · ${row.stage}` : ''}
          </span>
          <div className="sysline" style={{ flex: 1 }} />
        </div>
      )
    }
    if (row.type === 'toolgroup') {
      return <ToolGroupRow items={row.items} callIndex={callIndex} expanded={expanded.has(row.idx)} idx={row.idx} onToggle={toggleRow} fresh={isFreshExpand(row.idx)} />
    }
    if (row.type === 'sysgroup') {
      return <SysGroupRow items={row.items} expanded={expanded.has(row.idx)} idx={row.idx} onToggle={toggleRow} />
    }
    if (row.type === 'turnsummary') {
      return (
        <TurnSummaryRow row={row} expanded={expanded.has(row.idx)} idx={row.idx} onToggle={toggleRow}>
          {row.folded.map((r) => (
            <div key={r.idx} style={{ paddingLeft: 12 }}>{rowContent(r)}</div>
          ))}
        </TurnSummaryRow>
      )
    }
    return (
      <div className={flash === `item:${row.item.event.id}` ? 'flash-row' : ''}>
        <EventRow item={row.item} steered={steered} turnBoundary={turnBoundary} turnActive={turnActive} onJumpEvent={jumpToEvent} story={filter === 'story'} />
      </div>
    )
  }

  // 轨迹导出（US54）：kind 过滤随当前筛选档；Tauri 走保存对话框，浏览器 dev 合成 Blob 下载
  const doExport = async () => {
    const kinds = filter === 'messages' ? ['owner_message', 'agent_message']
      : filter === 'decisions' ? [...DECISION_KINDS]
      : undefined
    let count: number
    if (isTauri) {
      const { save } = await import('@tauri-apps/plugin-dialog')
      const path = await save({
        defaultPath: 'hexagon-trace.json',
        filters: [{ name: 'JSON', extensions: ['json'] }],
      })
      if (!path) return
      count = await api.exportEvents({ path, kinds })
    } else {
      const items = await api.exportTimelineItems(kinds)
      const doc = { format: 'hexagon-trace-export', version: 1, count: items.length, events: items.map((i) => i.event) }
      const a = document.createElement('a')
      a.href = URL.createObjectURL(new Blob([JSON.stringify(doc, null, 2)], { type: 'application/json' }))
      a.download = 'hexagon-trace.json'
      a.click()
      URL.revokeObjectURL(a.href)
      count = items.length
    }
    setExported(count)
    setTimeout(() => setExported(null), 2500)
  }

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', position: 'relative' }}>
      <StatusLine />
      <div style={{ display: 'flex', gap: 4, padding: '6px 14px 0', alignItems: 'center' }}>
        {(['all', 'messages', 'decisions', 'story'] as const).map((f) => (
          <button
            key={f}
            className={`btn ${filter === f ? 'primary' : ''}`}
            style={{ padding: '2px 10px', fontSize: 11 }}
            onClick={() => setFilter(f)}
          >
            {t(`timeline.filter${f[0].toUpperCase()}${f.slice(1)}`)}
          </button>
        ))}
        <div style={{ flex: 1 }} />
        {exported != null && (
          <span className="chip ok" style={{ fontSize: 10 }}>{t('timeline.exported', { count: exported })}</span>
        )}
        <button
          className="icon-btn"
          style={{ padding: '2px 6px', display: 'inline-flex', alignItems: 'center' }}
          title={t('timeline.export')}
          onClick={doExport}
        >
          <Icon name="export" size={12} />
        </button>
      </div>
      <div style={{ flex: 1, minHeight: 0, position: 'relative' }}>
        <div aria-busy={windowState.loading || !viewport.ready} style={{ height: '100%', visibility: viewport.ready ? 'visible' : 'hidden' }}>
        <Virtuoso
          key={windowState.generation}
          ref={viewport.ref}
          scrollerRef={viewport.bindScroller}
          firstItemIndex={viewport.firstItemIndex}
          computeItemKey={(_index, row) => rowKey(row)}
          totalListHeightChanged={viewport.onHeight}
          data={rows}
          // overscan：快速滚动时预渲视口上下各 600px，行不再贴边「凭空长出」
          increaseViewportBy={600}
          components={{ Footer: page?.has_after ? TimelineTail : StreamFooter }}
          // 不满一屏顶对齐（不要 alignToBottom）。贴底由 writeTail 写 scrollHeight，
          // followOutput 看不见 Footer 里的留白带，会把最新一行贴回输入框。
          followOutput={false}
          itemContent={(_i, row) => <div data-timeline-row={rowKey(row)} data-timeline-event={row.type === 'item' || row.type === 'chapter' ? row.idx : undefined} className={flash === rowKey(row) ? 'flash-row' : undefined}>{rowContent(row)}</div>}
        />
        </div>
        {!viewport.ready && <div role={windowState.error ? 'alert' : 'status'} style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', flexDirection: 'column', gap: 12, background: 'var(--bg-1)' }}>
          {windowState.error ? <><span>{t('timeline.loadFailed')}</span><button className="btn" title={`${t('timeline.retry')} ${formatBinding(bindingFor('timelineRetry'))}`} onClick={() => void windowState.retry()}>{t('timeline.retry')}</button></>
            : <LoadingState label={t('timeline.loadingRecent')} />}
        </div>}
        {/* Owner 2026-10-02: upward intent loads history; only progress or recovery needs a floating surface. */}
        {viewport.ready && (windowState.olderLoading || windowState.olderError || windowState.tailError) &&
          <div style={{ position: 'absolute', top: 4, left: '50%', transform: 'translateX(-50%)', zIndex: 21 }}>
            {windowState.olderLoading ? <LoadingState label={t('timeline.loadingOlder')} />
              : <button className="btn" title={`${t('timeline.retry')} ${formatBinding(bindingFor('timelineRetry'))}`}
                  onClick={() => void windowState.retry()}>
                  {windowState.tailError ? `${t('timeline.loadFailed')} · ${t('timeline.retry')}` : t('timeline.olderFailed')}</button>}
          </div>}

        {/* 松钉且高过窗口才显。不满一屏钉着、按钮藏着。 */}
        {viewport.ready && (page?.has_after || showStickButton(viewport.pinned, viewport.overflow)) && (
          <button
            className="btn primary"
            style={{ position: 'absolute', bottom: 12, right: 14, zIndex: 30, fontSize: 11, display: 'inline-flex', alignItems: 'center', gap: 4, borderRadius: 'var(--r-pill)', boxShadow: '0 4px 14px rgba(0,0,0,.3)' }}
            title={`${t('timeline.toBottom')} ${formatBinding(bindingFor('timelineLatest'))}`}
            onClick={viewport.holdPin}
          >
            <Icon name="arrow-down" size={11} />{unseen > 0 ? ` ${t('timeline.newEvents', { count: unseen })}` : ''}
          </button>
        )}
        {viewport.ready && <NodeRail marks={marks} onJump={jump} />}
      </div>
    </div>
  )
}
