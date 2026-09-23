import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { useUiStore } from '../store'
import { CreateRoleForm } from './RoleEditor'
import { Avatar } from './Avatar'
import { UsageTab } from './UsageTab'
import { FileTree } from './FileTree'
import { bindingFor, formatBinding } from '../keymap'
import { FileTypeIcon } from './FileTypeIcon'
import { Icon } from './Icon'
import { Row } from './Row'
import { slotLabel } from '../modelpick'
import { StageRail } from './StageBar'

function TeamRow({ m }: { m: { id: string; role: string; model_slot: string | null; status: string } }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const pv = useUiStore((s) => s.providers)
  return (
    <Row
      role="listitem"
      style={{ padding: '5px 12px', display: 'flex', gap: 8, alignItems: 'center', opacity: m.status === 'sleeping' ? 0.5 : 1, cursor: 'pointer' }}
      onClick={() => openTab({ id: `agent:${m.id}`, kind: 'agent', title: m.role, agentId: m.id, role: m.role })}
    >
      <Avatar agentId={m.id} role={m.role} size={22} />
      <span className={`dot ${m.status === 'active' ? 'on' : 'off'}`} />
      <span>{m.role}</span>
      <span className="dim3 mono" style={{ fontSize: 10, marginLeft: 'auto' }}>
        {slotLabel(m.model_slot, pv, t('agent.dedicatedTag'))}
      </span>
    </Row>
  )
}

export function SidePanel() {
  const { t } = useTranslation()
  const { invalidate } = useUiStore()
  const [creating, setCreating] = useState(false)
  const { artifacts, team, railOpen, setRailOpen, openTab, sideTab: tab, setSideTab: setTab } = useUiStore()
  const tip = formatBinding(bindingFor('toggleRail'))

  if (!railOpen) {
    return (
      <aside className="panel" style={{ width: 36, display: 'flex', flexDirection: 'column', alignItems: 'center', paddingTop: 8, gap: 10 }}>
        <button className="icon-btn" title={`${t('side.artifacts')} · ${tip}`} onClick={() => setRailOpen(true)} style={{ display: 'inline-flex', alignItems: 'center' }}>
          <Icon name="panel-right" size={14} />
        </button>
        <span className="chip ok" style={{ fontSize: 10 }}>{artifacts.length}</span>
      </aside>
    )
  }

  return (
    <aside className="panel" style={{ width: 260, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <div className="row-line" style={{ display: 'flex', alignItems: 'center', padding: '6px 6px 6px 8px', gap: 2, overflowX: 'auto' }}>
        {(['files', 'artifacts', 'team', 'usage', 'stages'] as const).map((k) => (
          <button
            key={k}
            data-side-tab={k}
            className="btn"
            style={{
              padding: '2px 6px', fontSize: 11, border: 'none', background: 'none', flexShrink: 0,
              color: tab === k ? 'var(--accent)' : 'var(--text-3)',
              fontWeight: tab === k ? 560 : 400,
            }}
            onClick={() => setTab(k)}
          >
            {t(`side.${k}`)}
          </button>
        ))}
        <div style={{ flex: 1 }} />
        <button className="icon-btn" title={tip} onClick={() => setRailOpen(false)} style={{ display: 'inline-flex', alignItems: 'center' }}>
          <Icon name="panel-right" size={12} />
        </button>
      </div>

      {tab === 'stages' && (
        <div style={{ flex: 1, overflowY: 'auto' }}>
          <StageRail />
        </div>
      )}

      {tab === 'artifacts' && (
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {/* ui-audit 票 06（P2-14）：空态给一句操作引导，不留纯白板 */}
          {artifacts.length === 0 && (
            <div className="dim3" style={{ padding: '14px 12px', fontSize: 11, lineHeight: 1.6 }}>
              {t('side.noArtifacts')}
            </div>
          )}
          {artifacts.map((a) => (
            <Row
              key={a.id}
              role="listitem"
              style={{ padding: '5px 12px', display: 'flex', gap: 8, alignItems: 'baseline', cursor: 'pointer' }}
              onClick={() => openTab({ id: `art:${a.path}`, kind: 'artifact', title: a.path, path: a.path })}
            >
              {/* 长名压缩 chip 的教训：flex 子项默认可缩，无 minWidth:0
                  的 nowrap 文本把标记挤成竖排——路径吃 flex:1+minWidth:0
                  截断，chip flexShrink:0+nowrap 保形；title 悬浮给全名。 */}
              <FileTypeIcon name={a.path.split('/').pop() ?? a.path} kind="file" size={13} style={{ alignSelf: 'center' }} />
              <span className="mono" title={a.path} style={{ flex: 1, minWidth: 0, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{a.path}</span>
              <span className={`chip ${a.status === 'stamped' ? 'amber' : a.status === 'superseded' ? '' : 'ok'}`} style={{ fontSize: 10, marginLeft: 'auto', flexShrink: 0, whiteSpace: 'nowrap' }}>
                {t(`side.${a.status}`, a.status)}
              </span>
            </Row>
          ))}
        </div>
      )}

      {tab === 'team' && (
        <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
          <div style={{ flex: 1 }}>
            {team.length === 0 && (
              <div className="dim3" style={{ padding: '14px 12px', fontSize: 11, lineHeight: 1.6 }}>
                {t('side.noMembers')}
              </div>
            )}
            {team.map((m) => (
              <TeamRow key={m.id} m={m} />
            ))}
          </div>
          <button
            className="btn"
            style={{ margin: '8px 12px', fontSize: 11 }}
            onClick={() => setCreating((v) => !v)}
          >
            {creating ? '−' : '+'} {t('agent.createRole')}
          </button>
          {creating && <CreateRoleForm onDone={() => { setCreating(false); invalidate('team') }} />}
        </div>
      )}

      {tab === 'usage' && <UsageTab />}
      {tab === 'files' && <FileTree />}
    </aside>
  )
}
