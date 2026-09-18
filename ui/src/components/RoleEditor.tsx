import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type AgentDetail, type RoleDef } from '../api'
import { useUiStore } from '../store'

const input: React.CSSProperties = {
  width: '100%', padding: '5px 8px', fontSize: 12,
  background: 'var(--bg-1)', border: '1px solid var(--bd)', borderRadius: 6,
  color: 'var(--text)', fontFamily: 'inherit',
}
const label: React.CSSProperties = { fontSize: 10, fontWeight: 560, color: 'var(--text-3)', marginTop: 8 }

/** 角色编辑器（票 30）：项目覆盖行 + 实例字段 + 授权名单。
 *  编辑后下次激活生效；globs/grants 即时按新值判。 */
export function RoleEditor({ agentId, onClose }: { agentId: string; onClose: () => void }) {
  const { t } = useTranslation()
  const { team, refresh } = useUiStore()
  const [d, setD] = useState<AgentDetail | null>(null)
  const [duty, setDuty] = useState('')
  const [reviewer, setReviewer] = useState('')
  const [slot, setSlot] = useState('')
  const [globs, setGlobs] = useState('')
  const [skills, setSkills] = useState('')
  const [mcpG, setMcpG] = useState('')
  const [skillG, setSkillG] = useState('')
  const [msg, setMsg] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api.agentDetail(agentId).then((v) => {
      setD(v)
      setDuty(v.def.duty)
      setReviewer(v.def.reviewer ?? '')
      setSlot(v.model_slot ?? v.def.model_slot)
      setGlobs(v.globs.join('\n'))
      setSkills(v.def.skills.join(', '))
      setMcpG(v.grants.filter((g) => g.kind === 'mcp').map((g) => g.name).join(', '))
      setSkillG(v.grants.filter((g) => g.kind === 'skill').map((g) => g.name).join(', '))
    }).catch((e) => setErr(String(e)))
  }, [agentId])

  const draft = async () => {
    setBusy(true); setErr(null)
    try {
      const text = await api.draftRoleDef(agentId, duty || ' ')
      if (text) setDuty(text)
    } catch (e) { setErr(String(e)) } finally { setBusy(false) }
  }

  const save = async () => {
    setBusy(true); setErr(null)
    try {
      await api.updateAgent(agentId, {
        duty,
        reviewer, // "" = 直达负责人
        model_slot: slot,
        skills: skills.split(',').map((s) => s.trim()).filter(Boolean),
        globs: globs.split('\n').map((s) => s.trim()).filter(Boolean),
      })
      await api.setAgentGrants(agentId, 'mcp', mcpG.split(',').map((s) => s.trim()).filter(Boolean))
      await api.setAgentGrants(agentId, 'skill', skillG.split(',').map((s) => s.trim()).filter(Boolean))
      await refresh()
      setMsg(t('agent.saved'))
      setTimeout(() => setMsg(null), 2500)
    } catch (e) { setErr(String(e)) } finally { setBusy(false) }
  }

  if (!d && !err) return <div className="dim3" style={{ padding: 12, fontSize: 11 }}>…</div>
  return (
    <div style={{ padding: '10px 14px', borderTop: '1px solid var(--bd)', background: 'var(--bg-1)' }}>
      {err && <div className="chip err" style={{ display: 'block', fontSize: 11, marginBottom: 8 }}>{err}</div>}
      {msg && <div className="chip ok" style={{ display: 'inline-block', fontSize: 11, marginBottom: 8 }}>{msg}</div>}
      {d?.custom && <div className="chip amber" style={{ fontSize: 10, marginBottom: 6 }}>{t('agent.customRole')}</div>}
      <div style={label}>{t('agent.duty')}</div>
      <textarea value={duty} onChange={(e) => setDuty(e.target.value)} rows={2} style={{ ...input, resize: 'vertical' }} />
      <div style={{ display: 'flex', gap: 8 }}>
        <div style={{ flex: 1 }}>
          <div style={label}>{t('agent.reviewer')}</div>
          <select value={reviewer} onChange={(e) => setReviewer(e.target.value)} style={input}>
            <option value="">{t('agent.noReviewer')}</option>
            {team.filter((m) => m.role !== d?.role).map((m) => (
              <option key={m.id} value={m.role}>{m.role}</option>
            ))}
          </select>
        </div>
        <div style={{ flex: 1 }}>
          <div style={label}>{t('agent.modelSlot')}</div>
          <input value={slot} onChange={(e) => setSlot(e.target.value)} style={input} />
        </div>
      </div>
      <div style={label}>{t('agent.globs')}</div>
      <textarea value={globs} onChange={(e) => setGlobs(e.target.value)} rows={2}
        placeholder="src/**" style={{ ...input, fontFamily: 'monospace', resize: 'vertical' }} />
      <div style={label}>{t('agent.skills')}</div>
      <input value={skills} onChange={(e) => setSkills(e.target.value)} placeholder="spec-writing, code-review" style={input} />
      <div style={label}>{t('agent.grantsMcp')}</div>
      <input value={mcpG} onChange={(e) => setMcpG(e.target.value)} placeholder="filesystem, github" style={input} />
      <div style={label}>{t('agent.grantsSkill')}</div>
      <input value={skillG} onChange={(e) => setSkillG(e.target.value)} placeholder="spec-writing" style={input} />
      <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
        <button className="btn primary" disabled={busy} onClick={save}>{t('agent.saveRole')}</button>
        <button className="btn" disabled={busy} onClick={draft}>{t('agent.draftAi')}</button>
        <button className="btn" onClick={onClose}>{t('agent.cancel')}</button>
      </div>
      <div className="dim3" style={{ fontSize: 10, marginTop: 6 }}>{t('agent.editHint')}</div>
    </div>
  )
}

/** 自建角色表单（票 30）：空表填 RoleDef → 校验入团队（休眠创建）。 */
export function CreateRoleForm({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation()
  const { team } = useUiStore()
  const [name, setName] = useState('')
  const [duty, setDuty] = useState('')
  const [reviewer, setReviewer] = useState('')
  const [slot, setSlot] = useState('default')
  const [globs, setGlobs] = useState('')
  const [skills, setSkills] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const create = async () => {
    setBusy(true); setErr(null)
    try {
      const def: RoleDef = {
        name: name.trim(), duty: duty.trim(),
        reviewer: reviewer || null, model_slot: slot.trim() || 'default',
        globs: globs.split('\n').map((s) => s.trim()).filter(Boolean),
        skills: skills.split(',').map((s) => s.trim()).filter(Boolean),
      }
      await api.createRole(def)
      onDone()
    } catch (e) { setErr(String(e)) } finally { setBusy(false) }
  }

  return (
    <div style={{ padding: '10px 12px', borderTop: '1px solid var(--bd)', background: 'var(--bg-1)' }}>
      {err && <div className="chip err" style={{ display: 'block', fontSize: 11, marginBottom: 8 }}>{err}</div>}
      <div style={label}>{t('agent.roleName')}</div>
      <input value={name} onChange={(e) => setName(e.target.value)} style={input} />
      <div style={label}>{t('agent.duty')}</div>
      <textarea value={duty} onChange={(e) => setDuty(e.target.value)} rows={2} style={{ ...input, resize: 'vertical' }} />
      <div style={{ display: 'flex', gap: 8 }}>
        <div style={{ flex: 1 }}>
          <div style={label}>{t('agent.reviewer')}</div>
          <select value={reviewer} onChange={(e) => setReviewer(e.target.value)} style={input}>
            <option value="">{t('agent.noReviewer')}</option>
            {team.map((m) => <option key={m.id} value={m.role}>{m.role}</option>)}
          </select>
        </div>
        <div style={{ flex: 1 }}>
          <div style={label}>{t('agent.modelSlot')}</div>
          <input value={slot} onChange={(e) => setSlot(e.target.value)} style={input} />
        </div>
      </div>
      <div style={label}>{t('agent.globs')}</div>
      <textarea value={globs} onChange={(e) => setGlobs(e.target.value)} rows={2}
        placeholder="docs/**" style={{ ...input, fontFamily: 'monospace', resize: 'vertical' }} />
      <div style={label}>{t('agent.skills')}</div>
      <input value={skills} onChange={(e) => setSkills(e.target.value)} style={input} />
      <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
        <button className="btn primary" disabled={busy || !name.trim()} onClick={create}>{t('agent.createRole')}</button>
        <button className="btn" onClick={onDone}>{t('agent.cancel')}</button>
      </div>
    </div>
  )
}
