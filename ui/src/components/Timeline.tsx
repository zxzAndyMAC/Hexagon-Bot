import { useTranslation } from 'react-i18next'
import { Virtuoso } from 'react-virtuoso'
import ReactMarkdown from 'react-markdown'
import { useUiStore } from '../store'
import { api, type TimelineItem } from '../api'

// 消息类型 → 渲染组件注册表：新事件类型注册即渲染（spec「时间线=投影」）
type Renderer = (item: TimelineItem) => React.ReactNode

function Bubble({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const authorId = item.message?.author ?? item.event.agent_id ?? '?'
  // agent_id → 角色名显示；owner 走本地化；角色名本身是项目数据不翻
  const author = authorId === 'owner' ? t('timeline.owner') : (team.find((m) => m.id === authorId)?.role ?? authorId)
  return (
    <div style={{ display: 'flex', gap: 8, padding: '4px 0' }}>
      <span className="chip" style={{ flexShrink: 0 }}>{author}</span>
      <div style={{ minWidth: 0 }}>
        <ReactMarkdown>{item.message?.body ?? ''}</ReactMarkdown>
      </div>
    </div>
  )
}

const renderers: Record<string, Renderer> = {
  agent_message: (it) => <Bubble item={it} />,
  owner_message: (it) => <Bubble item={it} />,
  stage_started: (it) => (
    <div
      className="sys-row"
      style={{ padding: '10px 0 4px', borderTop: '1px solid var(--border)', fontWeight: 510, color: 'var(--text-2)' }}
    >
      ▸ {String(it.event.payload.stage ?? '')}
    </div>
  ),
  artifact_delivered: (it) => (
    <div className="panel" style={{ padding: '6px 10px', margin: '3px 0' }}>
      <span className="chip ok">✓</span>{' '}
      <span className="mono">{String(it.event.payload.path ?? '')}</span>
      <span className="dim" style={{ fontSize: 11 }}>
        {' '}· {String(it.event.payload.kind ?? '')} v{String(it.event.payload.version ?? '')}
      </span>
    </div>
  ),
  flag_submitted: (it) => (
    <div className="card-flag" style={{ padding: '8px 12px', margin: '4px 0' }}>
      <div style={{ fontWeight: 510, color: 'var(--flag)' }}>⤺ {String(it.event.payload.target ?? '')}</div>
      <div className="dim" style={{ fontSize: 12 }}>{String(it.event.payload.reason ?? '')}</div>
    </div>
  ),
  review_passed: () => null, // 汇进阶段行
}

function Row({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const r = renderers[item.event.kind]
  if (r) {
    const n = r(item)
    if (n) return n
  }
  const who = item.event.agent_id
    ? team.find((m) => m.id === item.event.agent_id)?.role ?? item.event.agent_id
    : null
  return (
    <div className="sys-row" style={{ padding: '2px 0' }}>
      {t(`ev.${item.event.kind}`, { defaultValue: item.event.kind.replace(/_/g, ' ') })}
      {who ? ` · ${who}` : ''}
      {item.message?.body ? ` — ${item.message.body}` : ''}
    </div>
  )
}

export function Timeline() {
  const { t } = useTranslation()
  const { timeline, pending, refresh } = useUiStore()

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      {pending.map((q) => (
        <div key={q.id} className="card-ask" style={{ padding: '10px 14px', margin: '6px 14px 0' }}>
          <div style={{ fontWeight: 510, color: 'var(--accent)' }}>
            {q.payload.safety_net ? `⛔ ${t('cards.askSafetyNet')}` : `⚠ ${t('cards.ask')}`}
          </div>
          <div className="mono dim" style={{ fontSize: 12, margin: '4px 0' }}>
            {String(q.payload.tool ?? q.kind)} — {String(q.payload.reason ?? '')}
          </div>
          <div style={{ display: 'flex', gap: 8 }}>
            <button
              className="btn primary"
              onClick={async () => { await api.answerPermission(q.id, true); await refresh() }}
            >
              {t('cards.allowOnce')}
            </button>
            <button
              className="btn danger"
              onClick={async () => { await api.answerPermission(q.id, false); await refresh() }}
            >
              {t('cards.deny')}
            </button>
          </div>
        </div>
      ))}
      {timeline.length === 0 && pending.length === 0 ? (
        <div className="dim" style={{ padding: 24, textAlign: 'center' }}>{t('timeline.empty')}</div>
      ) : (
        <Virtuoso
          data={timeline}
          initialTopMostItemIndex={timeline.length - 1}
          itemContent={(_i, item) => <Row item={item} />}
          style={{ flex: 1, padding: '4px 14px' }}
        />
      )}
    </div>
  )
}
