import { useId, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import type { ContextSnapshot } from '../gen/ContextSnapshot'
import { fmtTime, parseTime } from '../usage'

export function ContextMeter({ agentIds = [] }: { agentIds?: string[] }) {
  const { t } = useTranslation()
  const tipId = useId()
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [dismissed, setDismissed] = useState(false)
  const open = (hovered || focused) && !dismissed
  const facts = useUiStore((s) => s.timelineFacts)
  const team = useUiStore((s) => s.team)
  const requests = useMemo(() => {
    const latest = new Map((facts?.latest_contexts ?? []).map(item => [item.agent_id, item]))
    const recent = [...latest.values()].sort((a, b) => b.event_id - a.event_id)[0]?.agent_id ?? null
    return { latest, recent }
  }, [facts])
  // Owner 2026-10-01: each role has its own request context. Never silently
  // substitute another role's usage when the addressed role has not run yet.
  const targets = agentIds.length ? [...new Set(agentIds)] : [requests.recent ?? '']
  const snapshots = targets.map((id) => {
    const event = requests.latest.get(id)
    const value: ContextSnapshot | null | undefined = event?.context
    const valid = value && [value.used_tokens, value.window_tokens, value.compact_at_tokens]
      .every((n) => typeof n === 'number' && Number.isFinite(n) && n >= 0) && value.window_tokens > 0
    return {
      id, value: valid ? value : null,
      role: team.find((member) => member.id === id)?.role ?? id,
      at: event?.created_at ?? '',
    }
  })
  const first = snapshots[0]?.value
  const percent = first ? Math.min(100, Math.round(first.used_tokens / first.window_tokens * 100)) : null
  return <div className="context-meter"
    onMouseEnter={() => { setHovered(true); setDismissed(false) }}
    onMouseLeave={() => setHovered(false)}
    onKeyDown={(event) => { if (event.key === 'Escape') { setDismissed(true); event.stopPropagation() } }}>
    <button type="button" className="context-meter-trigger" aria-label={t('context.title')}
      aria-describedby={open ? tipId : undefined}
      onFocus={() => { setFocused(true); setDismissed(false) }} onBlur={() => setFocused(false)}>
      <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">
        <circle cx="10" cy="10" r="7" fill="none" stroke="var(--border-strong)" strokeWidth="2" />
        {percent != null && <circle cx="10" cy="10" r="7" fill="none" stroke="currentColor" strokeWidth="2"
          pathLength="100" strokeDasharray={`${percent} 100`} transform="rotate(-90 10 10)" />}
      </svg>
      <span>{percent == null ? '—' : `${percent}%`}</span>
    </button>
    {open && <div id={tipId} role="tooltip" className="context-meter-detail">
      <strong>{t('context.title')}</strong>
      {snapshots.map(({ id, role, value, at }) => <div className="context-meter-role" key={id}>
        <div>{role}</div>
        {value ? <>
          {Number.isFinite(parseTime(at)) && <time dateTime={new Date(parseTime(at)).toISOString()}>{fmtTime(at)}</time>}
          <div>{t('context.usage', { used: value.used_tokens.toLocaleString(), total: value.window_tokens.toLocaleString() })}</div>
          <div>{t('context.compact', { count: value.compact_at_tokens.toLocaleString() })}</div>
        </> : <p>{t('context.empty')}</p>}
      </div>)}
      <p>{t('context.estimate')}</p>
    </div>}
  </div>
}
