import { useId, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import type { ContextSnapshot } from '../gen/ContextSnapshot'
import { parseTime } from '../usage'
import { Avatar } from '@/components/ui/avatar'

export function ContextMeter({ agentIds = [] }: { agentIds?: string[] }) {
  const { t } = useTranslation()
  const tipId = useId()
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const [dismissed, setDismissed] = useState(false)
  const open = (hovered || focused) && !dismissed
  const facts = useUiStore((s) => s.timelineFacts)
  const team = useUiStore((s) => s.team)
  const avatars = useUiStore((s) => s.avatars)
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
  const over = !!first && first.used_tokens >= first.compact_at_tokens
  const [pinned, setPinned] = useState(false)
  const shown = open || pinned
  // components D5（composer#mode）：22 圆环 + 11 等宽百分比；悬停 / 聚焦出 HoverCard 形详情，
  // 点击固定（补设计，便于键盘读取），Esc 收起。≥ 压缩阈值环色 --orange（只改色不改形）。
  return <div className="context-meter" data-open={shown || undefined}
    onMouseEnter={() => { setHovered(true); setDismissed(false) }}
    onMouseLeave={() => setHovered(false)}
    onKeyDown={(event) => { if (event.key === 'Escape') { setDismissed(true); setPinned(false); event.stopPropagation() } }}>
    <button type="button" className="context-meter-trigger" aria-label={t('context.title')} aria-expanded={pinned}
      aria-describedby={shown ? tipId : undefined}
      onClick={() => { setPinned(v => !v); setDismissed(false) }}
      onFocus={() => { setFocused(true); setDismissed(false) }} onBlur={() => { setFocused(false); setPinned(false) }}>
      <span className="cring" data-over={over || undefined} style={{ ['--p' as string]: percent ?? 0 }} aria-hidden />
      <span>{percent == null ? '—' : `${percent}%`}</span>
    </button>
    {shown && !dismissed && <div id={tipId} role="tooltip" className="context-meter-detail">
      {snapshots.map(({ id, role, value, at }) => {
        const pct = value ? Math.min(100, Math.round(value.used_tokens / value.window_tokens * 100)) : null
        const hot = !!value && value.used_tokens >= value.compact_at_tokens
        return <div className="context-meter-role" key={id}>
          <div className="context-meter-head">
            {id && <Avatar agentId={id} name={role} size={18} src={avatars[id]} />}
            <b>{t('context.heading', { role })}</b>
            {value && Number.isFinite(parseTime(at)) && <time dateTime={new Date(parseTime(at)).toISOString()}>{fmtClock(at)}</time>}
          </div>
          {value ? <>
            <div className="context-meter-bar" data-over={hot || undefined}><span><i style={{ width: `${pct}%` }} /></span><span>{pct}%</span></div>
            <dl className="context-meter-kv">
              <dt>{t('context.used')}</dt><dd>{fmtTokens(value.used_tokens)}</dd>
              <dt>{t('context.window')}</dt><dd>{fmtTokens(value.window_tokens)}</dd>
              <dt>{t('context.compactAt')}</dt><dd title={value.compact_at_tokens.toLocaleString()}>{value.compact_at_tokens.toLocaleString()}</dd>
            </dl>
          </> : <p>{t('context.empty')}</p>}
        </div>
      })}
      <p>{t('context.estimate')}</p>
    </div>}
  </div>
}

/** 等宽紧凑写法：84K / 1.2M（composer#mode 悬停卡）。 */
function fmtTokens(n: number) {
  if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${Math.round(n / 1_000)}K`
  return String(n)
}

/** 悬停卡头部只写时刻（HH:MM），日期留在 dateTime 属性里。 */
function fmtClock(at: string) {
  const d = new Date(parseTime(at))
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}
