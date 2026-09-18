import { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import { Avatar } from './Avatar'
import { EventRow, ToolGroupRow } from './Timeline'
import { buildRows } from '../timelineModel'

/** Agent 活动视图：头卡 + 按 agent_id 过滤的事件流。 */
export function AgentTab({ agentId }: { agentId: string }) {
  const { t } = useTranslation()
  const { team, timeline, refresh, openTab } = useUiStore()
  const member = team.find((m) => m.id === agentId)
  const fileRef = useRef<HTMLInputElement>(null)
  const [expanded, setExpanded] = useState<Set<number>>(new Set())

  const rows = useMemo(() => {
    const items = timeline.filter((it) => it.event.agent_id === agentId)
    return buildRows(items, 'all')
  }, [timeline, agentId])

  if (!member) return <div className="dim3" style={{ padding: 14 }}>{agentId}</div>

  const sleeping = member.status === 'sleeping'

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      {/* 头卡 */}
      <div className="row-line" style={{ padding: '10px 14px', display: 'flex', gap: 12, alignItems: 'center' }}>
        <button
          className="icon-btn"
          style={{ padding: 0, borderRadius: '26%', lineHeight: 0 }}
          title={t('agent.changeAvatar')}
          onClick={() => fileRef.current?.click()}
        >
          <Avatar agentId={agentId} role={member.role} size={40} />
        </button>
        <input
          ref={fileRef}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/gif"
          style={{ display: 'none' }}
          onChange={(e) => {
            const f = e.target.files?.[0]
            if (!f) return
            const r = new FileReader()
            r.onload = async () => {
              await api.setAgentAvatar(agentId, String(r.result))
              await refresh()
            }
            r.readAsDataURL(f)
            e.target.value = ''
          }}
        />
        <div>
          <div style={{ fontWeight: 560, fontSize: 14, display: 'flex', gap: 8, alignItems: 'center' }}>
            {member.role}
            <span className={`dot ${sleeping ? 'off' : 'on'}`} />
            <span className="dim3" style={{ fontSize: 11, fontWeight: 400 }}>
              {t(`side.${member.status}`, member.status)}
            </span>
          </div>
          <div className="dim3 mono" style={{ fontSize: 11, marginTop: 2 }}>
            {member.model_slot || '—'}
          </div>
        </div>
        <div style={{ flex: 1 }} />
        <button
          className={`btn ${sleeping ? 'primary' : ''}`}
          onClick={async () => { await api.setAgentSleeping(agentId, !sleeping); await refresh() }}
        >
          {sleeping ? t('agent.wake') : t('agent.sleep')}
        </button>
      </div>
      {/* 过滤事件流 */}
      <div style={{ flex: 1, overflowY: 'auto' }}>
        <div className="dim3" style={{ padding: '6px 14px', fontSize: 10, fontWeight: 560 }}>
          {t('agent.activity')}
        </div>
        {rows.length === 0 && <div className="dim3" style={{ padding: '4px 14px' }}>{t('agent.noEvents')}</div>}
        {rows.map((row) => {
          if (row.type === 'toolgroup') {
            return (
              <ToolGroupRow
                key={row.idx}
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
          const isArt = row.item.event.kind === 'artifact_delivered'
          return (
            <div
              key={row.idx}
              style={isArt ? { cursor: 'pointer' } : undefined}
              onClick={isArt
                ? () => {
                    const p = String(row.item.event.payload.path ?? '')
                    if (p) openTab({ id: `art:${p}`, kind: 'artifact', title: p, path: p })
                  }
                : undefined}
            >
              <EventRow item={row.item} />
            </div>
          )
        })}
      </div>
    </div>
  )
}
