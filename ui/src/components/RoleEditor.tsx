import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type AgentDetail, type ProvidersView, type RoleDef } from '../api'
import { useUiStore } from '../store'
import { dedicatedSlot, isDedicatedSlot, resolveBinding, sharedSlots, slotLabel } from '../modelpick'
import { EntityChips } from './EntityPicker'

// 逗号串 ↔ 字符串数组：名单字段持久化仍是 CSV（def.skills 数组落库前在此转换）
const csv = (s: string) => s.split(',').map((x) => x.trim()).filter(Boolean)

/* 表单控件两档密度（settings-density 01，owner 裁决）：sm=工作台紧凑（默认），
   md=设置页放大档。RoleEditor 同时被 AgentTab（工作台）与设置页团队分区复用，故密度走 props。 */
type Density = 'sm' | 'md'
// 凹槽井质感（ui-polish-2 ③）：--bg 底 + border-strong——bg-1 底在面板上隐形曾被 owner 点名
const inputSm: React.CSSProperties = {
  width: '100%', padding: '5px 8px', fontSize: 12,
  background: 'var(--bg)', border: '1px solid var(--border-strong)', borderRadius: 6,
  color: 'var(--text)', fontFamily: 'inherit',
}
const labelSm: React.CSSProperties = { fontSize: 10, fontWeight: 560, color: 'var(--text-3)', marginTop: 8 }
const fieldStyles = (density: Density) => ({
  input: density === 'md' ? { ...inputSm, padding: '7px 10px', fontSize: 13 } : inputSm,
  label: density === 'md' ? { ...labelSm, fontSize: 12, marginTop: 10 } : labelSm,
})

/** 角色编辑器（票 30）：项目覆盖行 + 实例字段 + 授权名单。
 *  编辑后下次激活生效；globs/grants 即时按新值判。 */
export function RoleEditor({ agentId, onClose, density = 'sm' }: { agentId: string; onClose: () => void; density?: Density }) {
  const { t } = useTranslation()
  const { input, label } = fieldStyles(density)
  const { team, invalidate } = useUiStore()
  const [d, setD] = useState<AgentDetail | null>(null)
  const [duty, setDuty] = useState('')
  const [reviewer, setReviewer] = useState('')
  const [slot, setSlot] = useState('')
  // ui-audit-2 票 01：模型选择 = 跟随共享槽 | 专属（供应商+模型 直选）。
  // 专属落库为 agent:<id> 命名槽（modelpick.ts 注释有否决方案记录）。
  const [mode, setMode] = useState<'follow' | 'dedicated'>('follow')
  const [pv, setPv] = useState<ProvidersView | null>(null)
  const [dPid, setDPid] = useState('')
  const [dModel, setDModel] = useState('')
  const [globs, setGlobs] = useState('')
  const [skills, setSkills] = useState('')
  const [mcpG, setMcpG] = useState('')
  const [skillG, setSkillG] = useState('')
  const [msg, setMsg] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    Promise.all([api.agentDetail(agentId), api.listProviders().catch(() => null)])
      .then(([v, doc]) => {
        setD(v)
        setPv(doc)
        setDuty(v.def.duty)
        setReviewer(v.def.reviewer ?? '')
        const s = v.model_slot ?? v.def.model_slot
        if (isDedicatedSlot(s)) {
          setMode('dedicated')
          const b = doc ? resolveBinding(doc.slots, s) : undefined
          setDPid(b?.provider_id ?? '')
          setDModel(b?.model ?? '')
        } else {
          setSlot(s)
        }
        setGlobs(v.globs.join('\n'))
        setSkills(v.def.skills.join(', '))
        setMcpG(v.grants.filter((g) => g.kind === 'mcp').map((g) => g.name).join(', '))
        setSkillG(v.grants.filter((g) => g.kind === 'skill').map((g) => g.name).join(', '))
      }).catch((e) => setErr(errText(e)))
  }, [agentId])

  const draft = async () => {
    setBusy(true); setErr(null)
    try {
      const text = await api.draftRoleDef(agentId, duty || ' ')
      if (text) setDuty(text)
    } catch (e) { setErr(errText(e)) } finally { setBusy(false) }
  }

  const save = async () => {
    setBusy(true); setErr(null)
    try {
      let modelSlot = slot
      if (mode === 'dedicated') {
        // 顺序有讲究：先绑 agent:<id> 槽（setSlotBinding 会热刷新 providers），
        // 再把 model_slot 指过去——反序会让解析瞬间落空到 default。
        if (!dPid || !dModel.trim()) {
          setErr(t('agent.modelIncomplete'))
          setBusy(false)
          return
        }
        const ds = dedicatedSlot(agentId)
        await api.setSlotBinding(ds, dPid, dModel.trim())
        modelSlot = ds
      } else if (isDedicatedSlot(d?.model_slot ?? '')) {
        // 从专属切回跟随：清掉孤儿 agent: 绑定，槽位表不留死条目。
        await api.removeSlotBinding(dedicatedSlot(agentId)).catch(() => {})
      }
      await api.updateAgent(agentId, {
        duty,
        reviewer, // "" = 直达负责人
        model_slot: modelSlot,
        skills: skills.split(',').map((s) => s.trim()).filter(Boolean),
        globs: globs.split('\n').map((s) => s.trim()).filter(Boolean),
      })
      await api.setAgentGrants(agentId, 'mcp', mcpG.split(',').map((s) => s.trim()).filter(Boolean))
      await api.setAgentGrants(agentId, 'skill', skillG.split(',').map((s) => s.trim()).filter(Boolean))
      await invalidate('team')
      setMsg(t('agent.saved'))
      setTimeout(() => setMsg(null), 2500)
    } catch (e) { setErr(errText(e)) } finally { setBusy(false) }
  }

  if (!d && !err) return <div className="dim3" style={{ padding: 12, fontSize: 11 }}>…</div>
  // ui-polish-3：右列详情统一 .panel 圆角边线卡（原 borderTop+bg-1 是底栏驻留期残留）
  return (
    <div className="panel" style={{ padding: '12px 14px' }}>
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
          <select value={mode} onChange={(e) => setMode(e.target.value as 'follow' | 'dedicated')} style={input}>
            <option value="follow">{t('agent.modelFollow')}</option>
            <option value="dedicated">{t('agent.modelDedicated')}</option>
          </select>
        </div>
      </div>
      {mode === 'follow' ? (
        <div>
          <select value={slot} onChange={(e) => setSlot(e.target.value)} style={input}>
            {sharedSlots(pv?.slots ?? {}, slot).map((s) => (
              <option key={s} value={s}>{slotLabel(s, pv, t('agent.dedicatedTag'))}</option>
            ))}
          </select>
          <div className="dim3" style={{ fontSize: 10, marginTop: 3 }}>{t('agent.modelFollowHint')}</div>
        </div>
      ) : (
        <div style={{ display: 'flex', gap: 8, marginTop: 4 }}>
          <select value={dPid} onChange={(e) => setDPid(e.target.value)} style={{ ...input, flex: 1 }}>
            <option value="">{t('agent.pickProvider')}</option>
            {(pv?.providers ?? []).filter((p) => p.enabled).map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
          <input
            value={dModel}
            onChange={(e) => setDModel(e.target.value)}
            placeholder={t('providers.model')}
            list="role-models"
            style={{ ...input, flex: 1 }}
          />
          <datalist id="role-models">
            {(pv?.providers.find((p) => p.id === dPid)?.models ?? []).map((m) => (
              <option key={m.id} value={m.id} />
            ))}
          </datalist>
        </div>
      )}
      <div style={label}>{t('agent.globs')}</div>
      <textarea value={globs} onChange={(e) => setGlobs(e.target.value)} rows={2}
        placeholder="src/**" style={{ ...input, fontFamily: 'monospace', resize: 'vertical' }} />
      <div style={label}>{t('agent.skills')}</div>
      <EntityChips value={csv(skills)} onChange={(ids) => setSkills(ids.join(', '))} source="skills" />
      <div style={label}>{t('agent.grantsMcp')}</div>
      <EntityChips value={csv(mcpG)} onChange={(ids) => setMcpG(ids.join(', '))} source="mcp" />
      <div style={label}>{t('agent.grantsSkill')}</div>
      <EntityChips value={csv(skillG)} onChange={(ids) => setSkillG(ids.join(', '))} source="skills" />
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
export function CreateRoleForm({ onDone, density = 'sm' }: { onDone: () => void; density?: Density }) {
  const { t } = useTranslation()
  const { input, label } = fieldStyles(density)
  const { team } = useUiStore()
  const [name, setName] = useState('')
  const [duty, setDuty] = useState('')
  const [reviewer, setReviewer] = useState('')
  const [slot, setSlot] = useState('default')
  const [globs, setGlobs] = useState('')
  const [skills, setSkills] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [pv, setPv] = useState<ProvidersView | null>(null)

  useEffect(() => {
    api.listProviders().then(setPv).catch(() => {})
  }, [])

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
    } catch (e) { setErr(errText(e)) } finally { setBusy(false) }
  }

  return (
    <div className="panel" style={{ padding: '12px 14px' }}>
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
          <select value={slot} onChange={(e) => setSlot(e.target.value)} style={input}>
            {sharedSlots(pv?.slots ?? {}, slot).map((s) => (
              <option key={s} value={s}>{slotLabel(s, pv, t('agent.dedicatedTag'))}</option>
            ))}
          </select>
          <div className="dim3" style={{ fontSize: 10, marginTop: 3 }}>{t('agent.modelFollowHint')}</div>
        </div>
      </div>
      <div style={label}>{t('agent.globs')}</div>
      <textarea value={globs} onChange={(e) => setGlobs(e.target.value)} rows={2}
        placeholder="docs/**" style={{ ...input, fontFamily: 'monospace', resize: 'vertical' }} />
      <div style={label}>{t('agent.skills')}</div>
      <EntityChips value={csv(skills)} onChange={(ids) => setSkills(ids.join(', '))} source="skills" />
      <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
        <button className="btn primary" disabled={busy || !name.trim()} onClick={create}>{t('agent.createRole')}</button>
        <button className="btn" onClick={onDone}>{t('agent.cancel')}</button>
      </div>
    </div>
  )
}
