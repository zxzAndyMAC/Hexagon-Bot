import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type AgentDetail, type ProvidersView, type RoleDef } from '../api'
import { useUiStore } from '../store'
import { dedicatedSlot, isDedicatedSlot, resolveBinding, sharedSlots, slotLabel } from '../modelpick'
import { EntityChips } from './EntityPicker'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, Input, Textarea } from '@/components/ui/input'
import { NativeSelect } from '@/components/ui/native-select'

// 逗号串 ↔ 字符串数组：名单字段持久化仍是 CSV（def.skills 数组落库前在此转换）
const csv = (s: string) => s.split(',').map((x) => x.trim()).filter(Boolean)

/* 表单控件两档密度（settings-density 01，owner 裁决）：sm=工作台紧凑（默认，侧栏「+ 添加角色」与画布 Agent 详情），
   md=设置页放大档。ui-redesign 票 11（components E4）：字段换成 A9 Field / Textarea / NativeSelect，按钮换 A1；
   密度只剩控件高度与按钮尺寸两档差别。 */
type Density = 'sm' | 'md'
const fieldCls = (density: Density) => density === 'md' ? '' : 'h-[30px] text-[12.5px]'
const btnSize = (density: Density) => (density === 'md' ? 'sm' : 'xs') as 'sm' | 'xs'

/** 角色编辑器（票 30）：项目覆盖行 + 实例字段 + 授权名单。
 *  编辑后下次激活生效；globs/grants 即时按新值判。 */
/** footStart：保存行最左的附加动作（gap D2：设置团队详情把「移出团队」放在这里，原型 settings#team）。 */
export function RoleEditor({ agentId, onClose, density = 'sm', footStart }: { agentId: string; onClose: () => void; density?: Density; footStart?: React.ReactNode }) {
  const { t } = useTranslation()
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

  if (!d && !err) return <div className="p-3 text-[12px] text-ink-3">…</div>
  const fc = fieldCls(density)
  const bs = btnSize(density)
  return (
    <div data-role-editor className="flex flex-col gap-3">
      {err && <div role="alert" className="text-[12.5px] text-red-ink">{err}</div>}
      {msg && <div role="status"><Badge variant="ok">{msg}</Badge></div>}
      {d?.custom && <div><Badge variant="info">{t('agent.customRole')}</Badge></div>}
      <Field label={t('agent.duty')}>
        <Textarea aria-label={t('agent.duty')} value={duty} onChange={(e) => setDuty(e.target.value)} />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label={t('agent.reviewer')}>
          <NativeSelect aria-label={t('agent.reviewer')} className={fc} value={reviewer} onChange={(e) => setReviewer(e.target.value)}>
            <option value="">{t('agent.noReviewer')}</option>
            {team.filter((m) => m.role !== d?.role).map((m) => (
              <option key={m.id} value={m.role}>{m.role}</option>
            ))}
          </NativeSelect>
        </Field>
        <Field label={t('agent.modelSlot')}>
          <NativeSelect aria-label={t('agent.modelSlot')} className={fc} value={mode} onChange={(e) => setMode(e.target.value as 'follow' | 'dedicated')}>
            <option value="follow">{t('agent.modelFollow')}</option>
            <option value="dedicated">{t('agent.modelDedicated')}</option>
          </NativeSelect>
        </Field>
      </div>
      {mode === 'follow' ? (
        <Field help={t('agent.modelFollowHint')}>
          <NativeSelect aria-label={t('agent.modelFollow')} className={fc} value={slot} onChange={(e) => setSlot(e.target.value)}>
            {sharedSlots(pv?.slots ?? {}, slot).map((s) => (
              <option key={s} value={s}>{slotLabel(s, pv, t('agent.dedicatedTag'))}</option>
            ))}
          </NativeSelect>
        </Field>
      ) : (
        <div className="grid grid-cols-2 gap-3">
          <NativeSelect aria-label={t('agent.pickProvider')} className={fc} value={dPid} onChange={(e) => setDPid(e.target.value)}>
            <option value="">{t('agent.pickProvider')}</option>
            {(pv?.providers ?? []).filter((p) => p.enabled).map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </NativeSelect>
          <Input
            aria-label={t('providers.model')}
            className={`${fc} font-mono`}
            value={dModel}
            onChange={(e) => setDModel(e.target.value)}
            placeholder={t('providers.model')}
            list="role-models"
          />
          <datalist id="role-models">
            {(pv?.providers.find((p) => p.id === dPid)?.models ?? []).map((m) => (
              <option key={m.id} value={m.id} />
            ))}
          </datalist>
        </div>
      )}
      <Field label={t('agent.globs')}>
        <Textarea aria-label={t('agent.globs')} className="font-mono text-[12px]" value={globs} onChange={(e) => setGlobs(e.target.value)} placeholder="src/**" />
      </Field>
      <RoleChips label={t('agent.skills')}><EntityChips value={csv(skills)} onChange={(ids) => setSkills(ids.join(', '))} source="skills" /></RoleChips>
      <RoleChips label={t('agent.grantsMcp')}><EntityChips value={csv(mcpG)} onChange={(ids) => setMcpG(ids.join(', '))} source="mcp" /></RoleChips>
      <RoleChips label={t('agent.grantsSkill')}><EntityChips value={csv(skillG)} onChange={(ids) => setSkillG(ids.join(', '))} source="skills" /></RoleChips>
      <div className="flex items-center gap-2">
        {footStart}
        <Button size={bs} variant="ghost" disabled={busy} onClick={draft}>{t('agent.draftAi')}</Button>
        <span className="flex-1" />
        <Button size={bs} variant="quiet" onClick={onClose}>{t('agent.cancel')}</Button>
        <Button size={bs} variant="primary" disabled={busy} loading={busy} onClick={save}>{t('agent.saveRole')}</Button>
      </div>
      <div className="text-[12px] text-ink-3">{t('agent.editHint')}</div>
    </div>
  )
}

/** 名单字段：label 12.5/500 + chip 行（E4「技能（chip 行 + 添加）」）。 */
function RoleChips({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col">
      <span className="mb-1.5 text-[12.5px] font-medium text-ink">{label}</span>
      {children}
    </div>
  )
}

/** 自建角色表单（票 30）：空表填 RoleDef → 校验入团队（休眠创建）。 */
export function CreateRoleForm({ onDone, density = 'sm' }: { onDone: () => void; density?: Density }) {
  const { t } = useTranslation()
  const { team } = useUiStore()
  const fc = fieldCls(density)
  const bs = btnSize(density)
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
    <div data-create-role className="flex flex-col gap-3">
      {err && <div role="alert" className="text-[12.5px] text-red-ink">{err}</div>}
      <Field label={t('agent.roleName')}>
        <Input aria-label={t('agent.roleName')} className={fc} value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label={t('agent.duty')}>
        <Textarea aria-label={t('agent.duty')} value={duty} onChange={(e) => setDuty(e.target.value)} />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label={t('agent.reviewer')}>
          <NativeSelect aria-label={t('agent.reviewer')} className={fc} value={reviewer} onChange={(e) => setReviewer(e.target.value)}>
            <option value="">{t('agent.noReviewer')}</option>
            {team.map((m) => <option key={m.id} value={m.role}>{m.role}</option>)}
          </NativeSelect>
        </Field>
        <Field label={t('agent.modelSlot')} help={t('agent.modelFollowHint')}>
          <NativeSelect aria-label={t('agent.modelSlot')} className={fc} value={slot} onChange={(e) => setSlot(e.target.value)}>
            {sharedSlots(pv?.slots ?? {}, slot).map((s) => (
              <option key={s} value={s}>{slotLabel(s, pv, t('agent.dedicatedTag'))}</option>
            ))}
          </NativeSelect>
        </Field>
      </div>
      <Field label={t('agent.globs')}>
        <Textarea aria-label={t('agent.globs')} className="font-mono text-[12px]" value={globs} onChange={(e) => setGlobs(e.target.value)} placeholder="docs/**" />
      </Field>
      <RoleChips label={t('agent.skills')}><EntityChips value={csv(skills)} onChange={(ids) => setSkills(ids.join(', '))} source="skills" /></RoleChips>
      <div className="flex items-center gap-2">
        <span className="flex-1" />
        <Button size={bs} variant="quiet" onClick={onDone}>{t('agent.cancel')}</Button>
        <Button size={bs} variant="primary" disabled={busy || !name.trim()} onClick={create}>{t('agent.createRole')}</Button>
      </div>
    </div>
  )
}
