import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso'
import { useTranslation } from 'react-i18next'
import type { TimelineItem } from '../api'
import { api, isTauri } from '../api'
import { Md, CodeBlock } from './Md'
import { useUiStore } from '../store'
import { buildRows, nodeMarks, deriveWorkbenchStatus, thinkingCollapsed, stampedByAutonomy, openTurn, DECISION_KINDS, SYS_HIGH_RISK, type Filter, type NodeMark, type Row as ModelRow } from '../timelineModel'
import { Avatar } from './Avatar'
import { Icon } from './Icon'
import { LoadingState } from './LoadingState'
import { Row } from './Row'
import { bindingFor, formatBinding } from '../keymap'
import { pairToolCalls, toolFilePath, toolInputSummary, TOOL_ICON, TOOL_LABEL, EXEC_CARD_TOOLS, type ToolCall } from '../agentSteps'
import { CallStatus, ToolExecCard } from './ExecCard'
import i18n from '../i18n'
import { fmtTime } from '../usage'
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
export function ThinkingRow({ text, live }: { text: string; live?: boolean }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
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
  if (!hasText) return null
  const { long, line } = thinkingCollapsed(trimmed)
  const shown = long && !open ? line : trimmed
  const endAt = end ?? (now > 0 ? now : null)
  const secs = start != null && endAt != null ? Math.max(0, (endAt - start) / 1000) : null
  const label = live
    ? `${t('timeline.thinking')} · ${Math.floor(secs ?? 0)}s`
    : timed && secs != null
      ? t('timeline.thought', { s: secs < 1 ? '<1' : String(Math.round(secs)) })
      : t('timeline.thinking')
  return (
    <button
      type="button"
      className={`thinking-row${long && !open ? ' thinking-ellipsis' : ''}`}
      data-thinking=""
      data-open={long && open ? '1' : '0'}
      aria-expanded={long ? open : undefined}
      onClick={() => { if (long) setOpen((v) => !v) }}
    >
      <span className={live ? 'shimmer-text' : undefined}>{label}</span>{' '}
      {long && open ? <span className="thinking-full">{shown}</span> : shown}
    </button>
  )
}

// 进行中的一条回复（hands-free 票 06）：增量追加在同一气泡，不另起多条。
export function LiveReply({
  role,
  text,
  thinking,
  generating,
  avatar,
}: {
  role: string
  text: string
  thinking: string
  generating: boolean
  avatar?: ReactNode
}) {
  const { t } = useTranslation()
  return (
    <div className="msg" data-live-reply="" style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 9 }}>
      {avatar}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, marginBottom: 3 }}>
          <span style={{ fontWeight: 560, fontSize: 12 }}>{role}</span>
          {generating && (
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
  const streams = useUiStore((s) => s.streams)
  const thinkings = useUiStore((s) => s.thinkings)
  const streamDone = useUiStore((s) => s.streamDone)
  const st = deriveWorkbenchStatus({ stages, team, timeline, streams, thinkings, streamDone })
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
  useEffect(() => {
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
  const highRisk = SYS_HIGH_RISK.has(sub)
  const autoStamp = item.event.kind === 'stamped' && stampedByAutonomy(item.event.payload)
  const label = autoStamp
    ? t('ev.stamped_auto', { stage: String(item.event.payload?.stage ?? '') })
    : sub
      ? trKey(t, `sys.${sub}`, sub)
      : trKey(t, `ev.${item.event.kind}`, item.event.kind)
  const note = item.event.kind === 'stamp_rejected' ? String(item.event.payload?.note ?? '') : ''
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

function StageHeader({ item }: { item: TimelineItem }) {
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
}

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

export function EventRow({
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
    const label = rejected
      ? t('timeline.routeRejected')
      : held
        ? t('timeline.routeHold')
        : t('timeline.routedTo', { role })
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
}

// ToolChips 移植（beautiful-ui 票 02，借形 MIT slev12397/beautiful-ui）：
// 折叠组展开成逐行 chip——图标+定名+mono 参数片，行点开看 input/result 明细，
// 组尾文件片点开产物 tab。原作的悬停 diff 预览未移植：core 把 fs_write 的
// content scrub 成 bytes（safety.rs），事件里没有增删行数据，不做假预览。
function ToolChipRow({ call, delay }: { call: ToolCall; delay: number }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const p = call.called.event.payload as Record<string, unknown>
  const tool = String(p.tool ?? '')
  const res = call.result?.event.payload as Record<string, unknown> | undefined
  const ok = res ? res.ok !== false : undefined
  const summary = toolInputSummary(p)
  const detail = JSON.stringify({ input: p.input ?? p, ...(res ? { result: res } : {}) }, null, 2)
  return (
    <div style={{ animation: `fade-up 300ms cubic-bezier(0.23,1,0.32,1) ${delay}ms both` }}>
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
          <CodeBlock code={detail} lang="json" />
        </div>
      )}
    </div>
  )
}

export function ToolGroupRow({ items, expanded, onToggle }: { items: TimelineItem[]; expanded: boolean; onToggle: () => void }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const openTab = useUiStore((s) => s.openTab)
  const agentId = items[0]?.event.agent_id
  const member = agentId ? team.find((x) => x.id === agentId) : undefined
  // 计数按调用对（called 吸收 result），不按事件条数——旧版 2N 的数会翻倍。
  const calls = useMemo(() => pairToolCalls(items), [items])
  const files = useMemo(() => {
    const seen = new Set<string>()
    for (const c of calls) {
      const path = toolFilePath(c.called.event.payload as Record<string, unknown>)
      if (path) seen.add(path)
    }
    return [...seen]
  }, [calls])
  return (
    <div>
      <div
        className="sysrow"
        style={{ cursor: 'pointer', userSelect: 'none' }}
        onClick={onToggle}
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
            return EXEC_CARD_TOOLS.has(tool)
              ? <ToolExecCard key={c.called.event.id} call={c} delay={Math.min(i, 12) * 45} />
              : <ToolChipRow key={c.called.event.id} call={c} delay={Math.min(i, 12) * 45} />
          })}
          {files.length > 0 && (
            <div className="tchip-files">
              {files.map((path, i) => (
                <button
                  key={path}
                  type="button"
                  className="chip chip-btn mono"
                  style={{ fontSize: 10, animation: `pop-in 250ms cubic-bezier(0.23,1,0.32,1) ${Math.min(i, 10) * 60}ms both` }}
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
}

export function SysGroupRow({ items, expanded, onToggle }: { items: TimelineItem[]; expanded: boolean; onToggle: () => void }) {
  const { t } = useTranslation()
  return (
    <div>
      <div
        className="sysrow"
        style={{ cursor: 'pointer', userSelect: 'none' }}
        onClick={onToggle}
      >
        <div className="sysline" />
        <span className="syslabel dim" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}>
          <Icon name="list" size={10} /> {t('timeline.sysEvents', { count: items.length })} <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={9} />
        </span>
      </div>
      {expanded && items.map((it) => <SystemRow key={it.event.id} item={it} />)}
    </div>
  )
}

// exec-cards 票 03：回合摘要行（Cursor Worked-for-Xs 借形）——已收束回合的
// 执行行收成单行（角色 · 工具数 · 耗时），点开就地展开还原被折行。
// 中性色：摘要是收纳不是信号；failed 回合挂 err 徽标（唯一需扫读区分的状态）。
function TurnSummaryRow({
  row,
  expanded,
  onToggle,
  children,
}: {
  row: Extract<ModelRow, { type: 'turnsummary' }>
  expanded: boolean
  onToggle: () => void
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
        onClick={onToggle}
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onToggle() } }}
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
      {expanded && children}
    </div>
  )
}

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
export function WaitingReply({ role, startedAt, avatar }: {
  role: string
  startedAt?: number | null
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
        <LoadingState label={t('timeline.working')} startedAt={startedAt ?? undefined} />
      </div>
    </div>
  )
}

function StreamFooter() {
  const { streams, thinkings, streamDone, team, timeline } = useUiStore()
  const ids = [...new Set([...Object.keys(streams), ...Object.keys(thinkings)])].filter((id) =>
    Object.values(streams[id] ?? {}).some((s) => s.length > 0) ||
    Object.values(thinkings[id] ?? {}).some((s) => s.length > 0),
  )
  const turn = openTurn(timeline)
  const waitingId = turn.agentId && !ids.includes(turn.agentId) ? turn.agentId : null
  const waitingMember = waitingId ? team.find((x) => x.id === waitingId) : undefined
  // Cursor 式底部留白（ui-polish-2 ⑤ 续，owner 二报「回不到底」）：
  // 列表尾部恒留空白带，最新内容可滚离输入条一段高度，而不是贴死在底缘。
  const tail = <div style={{ height: 96 }} />
  if (!ids.length && !waitingId) return tail
  return (
    <div>
      {waitingId && (
        <WaitingReply
          role={waitingMember?.role ?? waitingId}
          startedAt={turn.at}
          avatar={waitingMember ? <Avatar agentId={waitingId} role={waitingMember.role} size={34} /> : undefined}
        />
      )}
      {ids.map((id) => {
        const member = team.find((x) => x.id === id)
        const done = streamDone[id] != null
        return (
          <LiveReply
            key={id}
            role={member?.role ?? id}
            text={joinCalls(streams[id])}
            thinking={joinCalls(thinkings[id])}
            generating={!done}
            avatar={member ? <Avatar agentId={id} role={member.role} size={34} /> : undefined}
          />
        )
      })}
      {tail}
    </div>
  )
}

// ---- 主体 ----

export function Timeline() {
  const { t } = useTranslation()
  const { timeline, pending, streams, team } = useUiStore()
  const [filter, setFilter] = useState<Filter>('all')
  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [atBottom, setAtBottom] = useState(true)
  const [unseen, setUnseen] = useState(0)
  const [flash, setFlash] = useState<number | null>(null)
  const [exported, setExported] = useState<number | null>(null)
  const ref = useRef<VirtuosoHandle>(null)
  const scrollerEl = useRef<HTMLElement | null>(null)
  const prevLen = useRef(0)

  const rows = useMemo(() => buildRows(timeline, filter), [timeline, filter])
  const marks = useMemo(() => nodeMarks(timeline, rows, pending.length), [timeline, rows, pending.length])
  // 票 05：steering_injected 事件（payload.msg_id）标出已被本回合读到的
  // owner 消息；turnBoundary = 最近一次 turn_started，其后的 owner 消息
  // 才是「本回合内发来」的候选。
  const { steered, turnBoundary } = useMemo(() => {
    const s = new Set<number>()
    let boundary = -1
    for (const it of timeline) {
      if (it.event.kind === 'turn_started') boundary = it.event.id
      if (it.event.kind === 'system' && it.event.payload?.kind === 'steering_injected') {
        const mid = Number(it.event.payload.msg_id)
        if (mid) s.add(mid)
      }
    }
    return { steered: s, turnBoundary: boundary }
  }, [timeline])
  const turnActive = Object.keys(streams).length > 0

  useEffect(() => {
    if (timeline.length > prevLen.current && !atBottom) {
      setUnseen((u) => u + (timeline.length - prevLen.current))
    }
    prevLen.current = timeline.length
  }, [timeline.length, atBottom])

  // 钉底跟随流式增量（ui-polish-2 ⑤ 续，owner 二报「回不到底」）：流式文本
  // 只撑高 Footer，不在 data 里——Virtuoso 的 followOutput 看不见它，回底钮
  // 瞬跳后 Footer 继续长高便又离底。atBottom 期间每个 delta 手动钉到
  // scrollHeight；atBottomThreshold 放宽「在底部」判定抗闪烁。
  useEffect(() => {
    const el = scrollerEl.current
    if (atBottom && el) el.scrollTop = el.scrollHeight
  }, [streams, atBottom])

  const jump = (m: NodeMark) => {
    if (m.rowIdx < 0) {
      ref.current?.scrollToIndex({ index: 0, align: 'start' })
      return
    }
    ref.current?.scrollToIndex({ index: m.rowIdx, align: 'center', behavior: 'smooth' })
    const item = rows[m.rowIdx]
    const idx = item.type === 'item' ? item.item.event.id : item.idx
    setFlash(idx)
    setTimeout(() => setFlash(null), 1400)
  }

  // 票 16：按事件 id 跳转（return_summary 行回跳 since_event 锚点）。
  const jumpToEvent = (eid: number) => {
    const i = rows.findIndex((r) => {
      if (r.type === 'item') return r.item.event.id === eid
      if (r.type === 'toolgroup' || r.type === 'sysgroup') return r.items.some((x) => x.event.id === eid)
      // exec-cards 票 03：目标可能折在回合摘要行里
      if (r.type === 'turnsummary') {
        return r.folded.some((f) =>
          (f.type === 'toolgroup' || f.type === 'sysgroup') && f.items.some((x) => x.event.id === eid))
      }
      return false
    })
    if (i < 0) return
    const row = rows[i]
    // 命中折叠行先展开摘要行再跳，否则落点行不可见。
    if (row.type === 'turnsummary') setExpanded((s) => new Set(s).add(row.idx))
    jump({ rowIdx: i, icon: 'list', label: '' })
  }

  const toggleRow = (idx: number) => () =>
    setExpanded((s) => {
      const n = new Set(s)
      if (n.has(idx)) n.delete(idx); else n.add(idx)
      return n
    })

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
      return <ToolGroupRow items={row.items} expanded={expanded.has(row.idx)} onToggle={toggleRow(row.idx)} />
    }
    if (row.type === 'sysgroup') {
      return <SysGroupRow items={row.items} expanded={expanded.has(row.idx)} onToggle={toggleRow(row.idx)} />
    }
    if (row.type === 'turnsummary') {
      return (
        <TurnSummaryRow row={row} expanded={expanded.has(row.idx)} onToggle={toggleRow(row.idx)}>
          {row.folded.map((r) => (
            <div key={r.idx} style={{ paddingLeft: 12 }}>{rowContent(r)}</div>
          ))}
        </TurnSummaryRow>
      )
    }
    return (
      <div className={flash === row.item.event.id ? 'flash-row' : ''}>
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
      const items = timeline.filter((it) => !kinds || kinds.includes(it.event.kind))
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
            onClick={() => { setFilter(f); setUnseen(0) }}
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
        <Virtuoso
          ref={ref}
          scrollerRef={(el) => { scrollerEl.current = el as HTMLElement | null }}
          data={rows}
          components={{ Footer: StreamFooter }}
          atBottomThreshold={40}
          atBottomStateChange={(b) => { setAtBottom(b); if (b) setUnseen(0) }}
          followOutput={(isAtBottom) => (isAtBottom ? 'auto' : false)}
          itemContent={(_i, row) => rowContent(row)}
        />
        {/* ui-polish-2 ⑤：离开底部即给「回到底部」浮钮（不再只在新事件时），
            右下角落点不挡事件流；有新事件时附带计数文案 */}
        {!atBottom && (
          <button
            className="btn primary"
            style={{ position: 'absolute', bottom: 12, right: 14, zIndex: 30, fontSize: 11, display: 'inline-flex', alignItems: 'center', gap: 4, borderRadius: 'var(--r-pill)', boxShadow: '0 4px 14px rgba(0,0,0,.3)' }}
            title={t('timeline.toBottom')}
            onClick={() => {
              // scrollToIndex 只能对到末行末尾——列表最底部是 StreamFooter 流式区（行之外、可几百px高），
              // 且 smooth 滚动期间流还在长高会提前落点（owner 实测「回不到底」）。直接 scrollTop=scrollHeight 瞬跳。
              const el = scrollerEl.current
              if (el) el.scrollTo({ top: el.scrollHeight })
              else ref.current?.scrollToIndex({ index: rows.length - 1, align: 'end' })
              setUnseen(0)
            }}
          >
            <Icon name="arrow-down" size={11} />{unseen > 0 ? ` ${t('timeline.newEvents', { count: unseen })}` : ''}
          </button>
        )}
        <NodeRail marks={marks} onJump={jump} />
      </div>
    </div>
  )
}
