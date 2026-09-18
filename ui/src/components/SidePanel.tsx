import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import { Avatar } from './Avatar'
import { bindingFor, formatBinding } from '../keymap'

type Tab = 'artifacts' | 'team' | 'usage'

function TeamRow({ m }: { m: { id: string; role: string; model_slot: string | null; status: string } }) {
  const { t } = useTranslation()
  const refresh = useUiStore((s) => s.refresh)
  const fileRef = useRef<HTMLInputElement>(null)
  return (
    <div style={{ padding: '5px 12px', display: 'flex', gap: 8, alignItems: 'center', opacity: m.status === 'sleeping' ? 0.5 : 1 }}>
      <button
        className="icon-btn"
        style={{ padding: 0, borderRadius: '50%', lineHeight: 0 }}
        title={t('side.avatarTip')}
        onClick={() => fileRef.current?.click()}
      >
        <Avatar agentId={m.id} role={m.role} size={22} />
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
            await api.setAgentAvatar(m.id, String(r.result))
            await refresh()
          }
          r.readAsDataURL(f)
          e.target.value = ''
        }}
      />
      <span className={`dot ${m.status === 'active' ? 'on' : 'off'}`} />
      <span>{m.role}</span>
      <span className="dim3 mono" style={{ fontSize: 10, marginLeft: 'auto' }}>{m.model_slot || '—'}</span>
    </div>
  )
}

export function SidePanel() {
  const { t } = useTranslation()
  const { artifacts, team, usageTotal, railOpen, setRailOpen } = useUiStore()
  const [tab, setTab] = useState<Tab>('artifacts')
  const tip = formatBinding(bindingFor('toggleRail'))

  if (!railOpen) {
    return (
      <aside className="panel" style={{ width: 36, display: 'flex', flexDirection: 'column', alignItems: 'center', paddingTop: 8, gap: 10 }}>
        <button className="icon-btn" title={`${t('side.artifacts')} · ${tip}`} onClick={() => setRailOpen(true)} style={{ fontSize: 14 }}>
          ◧
        </button>
        <span className="chip ok" style={{ fontSize: 10 }}>{artifacts.length}</span>
      </aside>
    )
  }

  return (
    <aside className="panel" style={{ width: 260, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div className="row-line" style={{ display: 'flex', alignItems: 'center', padding: '6px 6px 6px 12px', gap: 2 }}>
        {(['artifacts', 'team', 'usage'] as const).map((k) => (
          <button
            key={k}
            className="btn"
            style={{
              padding: '2px 8px', fontSize: 11, border: 'none', background: 'none',
              color: tab === k ? 'var(--accent)' : 'var(--text-3)',
              fontWeight: tab === k ? 560 : 400,
            }}
            onClick={() => setTab(k)}
          >
            {t(`side.${k}`)}
          </button>
        ))}
        <div style={{ flex: 1 }} />
        <button className="icon-btn" title={tip} onClick={() => setRailOpen(false)} style={{ fontSize: 12 }}>
          ◨
        </button>
      </div>

      {tab === 'artifacts' && (
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {artifacts.map((a) => (
            <div key={a.id} style={{ padding: '5px 12px', display: 'flex', gap: 8, alignItems: 'baseline' }}>
              <span className="mono" style={{ fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{a.path}</span>
              <span className={`chip ${a.status === 'stamped' ? 'amber' : a.status === 'superseded' ? '' : 'ok'}`} style={{ fontSize: 10, marginLeft: 'auto' }}>
                {t(`side.${a.status}`, a.status)}
              </span>
            </div>
          ))}
        </div>
      )}

      {tab === 'team' && (
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {team.map((m) => (
            <TeamRow key={m.id} m={m} />
          ))}
        </div>
      )}

      {tab === 'usage' && (
        <div style={{ flex: 1, overflowY: 'auto', padding: '8px 12px', fontSize: 12 }}>
          <div className="dim" style={{ marginBottom: 6 }}>{t('topbar.usage')}</div>
          <div className="mono">
            {usageTotal ? `¥${(usageTotal.spent_mc / 100000).toFixed(2)}${usageTotal.limit_cents ? ` / ¥${(usageTotal.limit_cents / 100).toFixed(2)}` : ''}` : '—'}
          </div>
        </div>
      )}
    </aside>
  )
}
