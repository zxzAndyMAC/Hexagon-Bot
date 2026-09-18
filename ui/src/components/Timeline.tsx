import { useEffect, useMemo, useRef, useState } from 'react'
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso'
import ReactMarkdown from 'react-markdown'
import { useTranslation } from 'react-i18next'
import type { TimelineItem } from '../api'
import { useUiStore } from '../store'
import { buildRows, nodeMarks, type Filter, type NodeMark } from '../timelineModel'
import { Avatar } from './Avatar'

// ---- 渲染 ----

function fmtTime(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

const chipCls = (s: string) =>
  s === 'passed' ? 'ok' : s === 'queued' ? 'warn' : s === 'rejected' || s === 'failed' ? 'err' : 'err'

function SystemRow({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  return (
    <div className="sysrow">
      <div className="sysline" />
      <span className="syslabel">
        {t(`ev.${item.event.kind}`, { defaultValue: item.event.kind.replace(/_/g, ' ') })}
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
      <span style={{ color: 'var(--accent)', fontSize: 11 }}>◆</span>
      <span style={{ fontWeight: 560, fontSize: 13 }}>{String(p.stage ?? '')}</span>
      {n > 0 && <span className="chip ok">{n} {t('side.artifacts').toLowerCase()}</span>}
      <div className="sysline" style={{ flex: 1 }} />
    </div>
  )
}

function ReturnSummaryRow({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const p = item.event.payload
  const events = Number(p.events ?? 0)
  const pending = Number(p.pending_todos ?? 0)
  const groups = p as Record<string, Record<string, unknown>>
  const lines = (['artifacts', 'flags', 'permissions', 'stages'] as const).flatMap((g) => {
    const m = groups[g]
    return m ? Object.entries(m).filter(([, v]) => Number(v) > 0).map(([k, v]) => `${g}.${k}: ${v}`) : []
  })
  return (
    <div className="card-ask" style={{ margin: '6px 14px', padding: '8px 14px', cursor: 'pointer' }} onClick={() => setOpen(!open)}>
      <div style={{ fontWeight: 510, color: 'var(--accent)', fontSize: 12 }}>
        ☰ {t('cards.returnSummary')} · {events} {open ? '▾' : '▸'}
        {pending > 0 && <span className="chip amber" style={{ marginLeft: 8 }}>{t('cards.pending', { count: pending })}</span>}
      </div>
      {open && (
        <div className="mono dim" style={{ fontSize: 11, marginTop: 6, whiteSpace: 'pre-wrap' }}>
          {lines.length ? lines.join('\n') : '—'}
        </div>
      )}
    </div>
  )
}

export function EventRow({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const openTab = useUiStore((s) => s.openTab)
  const k = item.event.kind
  if (k === 'stage_started') return <StageHeader item={item} />
  if (k === 'return_summary') return <ReturnSummaryRow item={item} />
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
          {path}
          <span className={`chip ${chipCls(status)}`}>{status}</span>
        </span>
      </div>
    )
  }
  if (k === 'flag_submitted' || k === 'flag_adjudicated') {
    const ev = item.event
    const severity = String(ev.payload.severity ?? '')
    return (
      <div className="card card-flag" style={{ margin: '4px 14px' }}>
        <div style={{ fontSize: 12 }}>
          <span className={`chip ${chipCls(severity)}`} style={{ marginRight: 8 }}>{severity}</span>
          {String(ev.payload.target ?? '')}
          {ev.payload.section ? ` · ${String(ev.payload.section)}` : ''}
        </div>
        <div className="dim" style={{ marginTop: 4, fontSize: 12 }}>{String(ev.payload.reason ?? '')}</div>
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
        <div className="msg" style={{ alignItems: 'flex-end' }}>
          <div className="msg-author dim" style={{ textAlign: 'right' }}>
            {title}{time && <span className="dim3" style={{ marginLeft: 6 }}>{time}</span>}
          </div>
          <div className="msg-body"><ReactMarkdown>{m.body}</ReactMarkdown></div>
        </div>
      )
    }
    return (
      <div className="msg" style={{ flexDirection: 'row', alignItems: 'flex-start', gap: 9 }}>
        {member && <Avatar agentId={m.author} role={member.role} size={34} />}
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 8, marginBottom: 3 }}>
            <span style={{ fontWeight: 560, fontSize: 12 }}>{title}</span>
            {time && <span className="dim3" style={{ fontSize: 10 }}>{time}</span>}
          </div>
          <div className="msg-body"><ReactMarkdown>{m.body}</ReactMarkdown></div>
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
          ⚙ {t('timeline.toolCalls', { count: items.length })} {expanded ? '▾' : '▸'}
        </span>
      </div>
      {expanded && items.map((it) => (
        <div key={it.event.id} className="sysrow" style={{ paddingLeft: 24 }}>
          <div className="sysline" />
          <span className="syslabel dim3" style={{ fontSize: 10 }}>
            {it.event.kind === 'tool_called'
              ? `→ ${String(it.event.payload.tool ?? '')}`
              : `← ${it.event.kind}`}
          </span>
        </div>
      ))}
    </div>
  )
}

// ---- 语义节点轨（hover 展开 / 点击跳转 / 待决脉动）----

function NodeRail({ marks, onJump }: { marks: NodeMark[]; onJump: (m: NodeMark) => void }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
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
        background: open ? 'var(--bg-1)' : 'transparent',
        borderLeft: open ? '1px solid var(--border)' : 'none',
      }}
    >
      {!open ? (
        <div style={{ width: 14, height: '100%', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6, paddingTop: 40, overflow: 'hidden' }}>
          {marks.map((m, i) => (
            <span key={i} style={{ fontSize: 8, color: m.pending ? 'var(--warn)' : 'var(--fg-3)' }}>{m.icon}</span>
          ))}
        </div>
      ) : (
        <div style={{ flex: 1, overflowY: 'auto', padding: '8px 8px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
            <span className="dim3" style={{ fontSize: 10, fontWeight: 560 }}>{t('rail.nodes')}</span>
            <button className="icon-btn" style={{ fontSize: 10 }} onClick={() => setOpen(false)}>✕</button>
          </div>
          {marks.map((m, i) => (
            <div
              key={i}
              onClick={() => { onJump(m); setOpen(false) }}
              style={{
                display: 'flex', gap: 6, alignItems: 'baseline', padding: '3px 4px', borderRadius: 4,
                cursor: 'pointer', fontSize: 11,
                color: m.pending ? 'var(--warn)' : 'var(--fg-1)',
                animation: m.pending ? 'pulse-amber 1.6s infinite' : undefined,
              }}
            >
              <span style={{ flexShrink: 0 }}>{m.icon}</span>
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.label}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

// ---- 主体 ----

export function Timeline() {
  const { t } = useTranslation()
  const { timeline, pending } = useUiStore()
  const [filter, setFilter] = useState<Filter>('all')
  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [atBottom, setAtBottom] = useState(true)
  const [unseen, setUnseen] = useState(0)
  const [flash, setFlash] = useState<number | null>(null)
  const ref = useRef<VirtuosoHandle>(null)
  const prevLen = useRef(0)

  const rows = useMemo(() => buildRows(timeline, filter), [timeline, filter])
  const marks = useMemo(() => nodeMarks(timeline, rows, pending.length), [timeline, rows, pending.length])

  useEffect(() => {
    if (timeline.length > prevLen.current && !atBottom) {
      setUnseen((u) => u + (timeline.length - prevLen.current))
    }
    prevLen.current = timeline.length
  }, [timeline.length, atBottom])

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

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', position: 'relative' }}>
      <div style={{ display: 'flex', gap: 4, padding: '6px 14px 0', alignItems: 'center' }}>
        {(['all', 'messages', 'decisions'] as const).map((f) => (
          <button
            key={f}
            className={`btn ${filter === f ? 'primary' : ''}`}
            style={{ padding: '2px 10px', fontSize: 11 }}
            onClick={() => { setFilter(f); setUnseen(0) }}
          >
            {t(`timeline.filter${f[0].toUpperCase()}${f.slice(1)}`)}
          </button>
        ))}
      </div>
      <div style={{ flex: 1, minHeight: 0, position: 'relative' }}>
        <Virtuoso
          ref={ref}
          data={rows}
          atBottomStateChange={(b) => { setAtBottom(b); if (b) setUnseen(0) }}
          followOutput={(isAtBottom) => (isAtBottom ? 'auto' : false)}
          itemContent={(_i, row) => {
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
            return (
              <div className={flash === row.item.event.id ? 'flash-row' : ''}>
                <EventRow item={row.item} />
              </div>
            )
          }}
        />
        {unseen > 0 && (
          <button
            className="btn primary"
            style={{ position: 'absolute', bottom: 12, left: '50%', transform: 'translateX(-50%)', zIndex: 30, fontSize: 11 }}
            onClick={() => { ref.current?.scrollToIndex({ index: rows.length - 1, behavior: 'smooth' }); setUnseen(0) }}
          >
            ↓ {t('timeline.newEvents', { count: unseen })}
          </button>
        )}
        <NodeRail marks={marks} onJump={jump} />
      </div>
    </div>
  )
}
