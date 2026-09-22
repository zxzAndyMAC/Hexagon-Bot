import { useEffect, useMemo, useRef, useState } from 'react'
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso'
import { useTranslation } from 'react-i18next'
import type { TimelineItem } from '../api'
import { api, isTauri } from '../api'
import { Md } from './Md'
import { useUiStore } from '../store'
import { buildRows, nodeMarks, DECISION_KINDS, SYS_HIGH_RISK, type Filter, type NodeMark } from '../timelineModel'
import { Avatar } from './Avatar'
import { Icon } from './Icon'
import { Row } from './Row'
import { bindingFor, formatBinding } from '../keymap'
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

// ui-audit 票 11（P3-23）：超高消息体渐隐夹持 + 展开/收起。
// 原实现 max-height+内滚——滚到底才看得见尾巴，扫读时不知道藏了内容。
function CollapsibleBody({ text, unclamped }: { text: string; unclamped?: boolean }) {
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
      <div ref={ref} className={`msg-body ${tall && !open ? 'clamped' : ''}`}>
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
  const label = sub
    ? trKey(t, `sys.${sub}`, sub)
    : trKey(t, `ev.${item.event.kind}`, item.event.kind)
  return (
    <div className="sysrow">
      <div className="sysline" />
      <span className="syslabel" style={highRisk ? { color: 'var(--err)' } : undefined}>
        {label}
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
          <CollapsibleBody text={m.body} unclamped={story} />
        </div>
      </div>
    )
  }
  return <SystemRow item={item} />
}

export function ToolGroupRow({ items, expanded, onToggle }: { items: TimelineItem[]; expanded: boolean; onToggle: () => void }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const agentId = items[0]?.event.agent_id
  const member = agentId ? team.find((x) => x.id === agentId) : undefined
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
          <Icon name="tool" size={10} /> {t('timeline.toolCalls', { count: items.length })} <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={9} />
        </span>
      </div>
      {expanded && items.map((it) => (
        <div key={it.event.id} className="sysrow" style={{ paddingLeft: 24 }}>
          <div className="sysline" />
          <span className="syslabel dim3" style={{ fontSize: 10, display: 'inline-flex', alignItems: 'center', gap: 4 }}>
            {it.event.kind === 'tool_called'
              ? <><Icon name="arrow-right" size={9} /> {String(it.event.payload.tool ?? '')}</>
              : <><Icon name="arrow-left" size={9} /> {trKey(t, `ev.${it.event.kind}`, it.event.kind)}</>}
          </span>
        </div>
      ))}
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
function StreamFooter() {
  const { t } = useTranslation()
  const { streams, streamDone, team } = useUiStore()
  const ids = Object.keys(streams).filter((id) =>
    Object.values(streams[id]).some((s) => s.length > 0),
  )
  // Cursor 式底部留白（ui-polish-2 ⑤ 续，owner 二报「回不到底」）：
  // 列表尾部恒留空白带，最新内容可滚离输入条一段高度，而不是贴死在底缘。
  const tail = <div style={{ height: 96 }} />
  if (!ids.length) return tail
  return (
    <div>
      {ids.map((id) => {
        const member = team.find((x) => x.id === id)
        const done = streamDone[id] != null
        const text = Object.keys(streams[id])
          .map(Number)
          .sort((a, b) => a - b)
          .map((c) => streams[id][c])
          .filter((s) => s.length > 0)
          .join('\n\n')
        return (
          <div className="msg" key={id} style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 9 }}>
            {member && <Avatar agentId={id} role={member.role} size={34} />}
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, marginBottom: 3 }}>
                <span style={{ fontWeight: 560, fontSize: 12 }}>{member?.role ?? id}</span>
                {!done && (
                  <span className="dim3" style={{ fontSize: 10 }}>{t('timeline.streaming')}</span>
                )}
              </div>
              <CollapsibleBody text={text} />
            </div>
          </div>
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
    const i = rows.findIndex((r) =>
      r.type === 'item' ? r.item.event.id === eid
      : r.type === 'chapter' ? false
      : r.items.some((x) => x.event.id === eid))
    if (i >= 0) jump({ rowIdx: i, icon: 'list', label: '' })
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
          itemContent={(_i, row) => {
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
              return (
                <ToolGroupRow
                  items={row.items}
                  expanded={expanded.has(row.idx)}
                  onToggle={() => setExpanded((s) => {
                    const n = new Set(s)
                    if (n.has(row.idx)) n.delete(row.idx); else n.add(row.idx)
                    return n
                  })}
                />
              )
            }
            if (row.type === 'sysgroup') {
              return (
                <SysGroupRow
                  items={row.items}
                  expanded={expanded.has(row.idx)}
                  onToggle={() => setExpanded((s) => {
                    const n = new Set(s)
                    if (n.has(row.idx)) n.delete(row.idx); else n.add(row.idx)
                    return n
                  })}
                />
              )
            }
            return (
              <div className={flash === row.item.event.id ? 'flash-row' : ''}>
                <EventRow item={row.item} steered={steered} turnBoundary={turnBoundary} turnActive={turnActive} onJumpEvent={jumpToEvent} story={filter === 'story'} />
              </div>
            )
          }}
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
