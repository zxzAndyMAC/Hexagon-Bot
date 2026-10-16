// 设置「团队」分区（ui-audit-2 票 02 / global-config 票 02 / settings-3col 票 03；ui-redesign 票 11 换皮：
// settings#team，components E2 + E4）。有项目时列表头是分段「本项目 Agent | 模板库」；无项目时只有模板库
// （模板是可复用配置不是项目状态，ADR 0057；建项目时按名复制物化，不回写）。
// 项目 Agent 详情仍是 RoleEditor（保存制：设置里改的是正在运行的项目，ADR 0091 的即改即生效只管向导）。
// 「从模板添加」在详情位出模板选择面板（选中即 createRole(def) 复制物化进项目）。
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type AgentDetail, type ProvidersView, type RoleDef, type RoleTemplate, type TeamRow } from '../../api'
import { useUiStore } from '../../store'
import { sharedSlots, slotLabel } from '../../modelpick'
import { Avatar } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, Input, Textarea } from '@/components/ui/input'
import { NativeSelect } from '@/components/ui/native-select'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { EntityChips } from '../EntityPicker'
import { Icon } from '../Icon'
import { CreateRoleForm, RoleEditor } from '../RoleEditor'
import { DetailHead, Empty, ListDetail } from './kit'

/** 模板编辑表单（global-config 票 02）：内置行「编辑」= 同名覆盖层创建（改名即另存副本）；
 *  自定义行可直接改可删。校验交给后端 fail-closed。 */
function TemplateEditor({ def, names, isCustom, onDone }: {
  def: RoleDef
  names: string[]
  isCustom: boolean
  onDone: () => void
}) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const askConfirm = useUiStore((s) => s.askConfirm)
  const [name, setName] = useState(def.name)
  const [duty, setDuty] = useState(def.duty)
  const [reviewer, setReviewer] = useState(def.reviewer ?? '')
  const [slot, setSlot] = useState(def.model_slot)
  const [globs, setGlobs] = useState(def.globs.join('\n'))
  const [skills, setSkills] = useState<string[]>(def.skills)
  const [busy, setBusy] = useState(false)
  const [pv, setPv] = useState<ProvidersView | null>(null)

  useEffect(() => { api.listProviders().then(setPv).catch(() => {}) }, [])

  const save = async () => {
    setBusy(true)
    try {
      await api.saveRoleTemplate({
        name: name.trim(), duty: duty.trim(),
        reviewer: reviewer || null, model_slot: slot.trim() || 'default',
        globs: globs.split('\n').map((s) => s.trim()).filter(Boolean),
        skills,
      })
      onDone()
    } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
  }

  const del = () =>
    askConfirm({
      title: t('teamTpl.confirmDel', { name: def.name }),
      danger: true,
      run: async () => {
        setBusy(true)
        try {
          await api.deleteRoleTemplate(def.name)
          onDone()
        } catch (e) { pushToast(errText(e), 'err') } finally { setBusy(false) }
      },
    })

  return (
    <div data-template-editor className="flex max-w-[520px] flex-col gap-3">
      <DetailHead
        lead={<Avatar name={name || '?'} size={40} />}
        title={def.name || t('teamTpl.newTpl')}
        sub={isCustom ? t('teamTpl.custom') : t('teamTpl.builtin')}
      />
      <Field label={t('agent.roleName')} help={!isCustom ? t('teamTpl.builtinHint') : undefined}>
        <Input aria-label={t('agent.roleName')} value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label={t('agent.duty')}>
        <Textarea aria-label={t('agent.duty')} value={duty} onChange={(e) => setDuty(e.target.value)} />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label={t('agent.reviewer')}>
          <NativeSelect aria-label={t('agent.reviewer')} value={reviewer} onChange={(e) => setReviewer(e.target.value)}>
            <option value="">{t('agent.noReviewer')}</option>
            {names.filter((n) => n !== def.name).map((n) => <option key={n} value={n}>{n}</option>)}
          </NativeSelect>
        </Field>
        <Field label={t('agent.modelSlot')}>
          <NativeSelect aria-label={t('agent.modelSlot')} value={slot} onChange={(e) => setSlot(e.target.value)}>
            {sharedSlots(pv?.slots ?? {}, slot).map((s) => (
              <option key={s} value={s}>{slotLabel(s, pv, t('agent.dedicatedTag'))}</option>
            ))}
          </NativeSelect>
        </Field>
      </div>
      <Field label={t('agent.globs')}>
        <Textarea aria-label={t('agent.globs')} className="font-mono text-[12px]" placeholder="src/**" value={globs} onChange={(e) => setGlobs(e.target.value)} />
      </Field>
      <div className="flex flex-col">
        <span className="mb-1.5 text-[12.5px] font-medium text-ink">{t('agent.skills')}</span>
        <EntityChips value={skills} onChange={setSkills} source="skills" />
      </div>
      <div className="mt-1 flex items-center gap-2">
        {isCustom && (
          <Button size="sm" variant="ghost" className="text-red" disabled={busy} onClick={del}>
            <Icon name="trash" size={14} />{t('teamTpl.delTpl')}
          </Button>
        )}
        <span className="flex-1" />
        <Button size="sm" variant="quiet" onClick={onDone}>{t('agent.cancel')}</Button>
        <Button size="sm" variant="primary" disabled={busy || !name.trim()} loading={busy} onClick={save}>{t('teamTpl.saveTpl')}</Button>
      </div>
    </div>
  )
}

function useTemplates() {
  const [tpls, setTpls] = useState<RoleTemplate[] | null>(null)
  const [error, setError] = useState('')
  const load = useCallback(() => {
    setError('')
    api.listRoleTemplates().then(setTpls).catch((e) => { setTpls([]); setError(errText(e)) })
  }, [])
  return { tpls, error, load }
}

/** 团队详情副行「来自模板「X」· 本项目定制」（原型 settings#team；gap D2）。按名对模板库：
 *  同名模板存在 → 来自模板，且职责/审阅/模型槽/技能任一不同 → 追加「本项目定制」；
 *  无同名模板 → 本项目自建。模板在建项目时按名复制物化（ADR 0057），不存来源指针，按名比是唯一依据。 */
export function templateOrigin(
  role: string,
  def: AgentDetail['def'] | null,
  tpls: RoleTemplate[] | null,
  t: (k: string, o?: Record<string, unknown>) => string,
): string | null {
  if (!tpls || !def) return null
  const tp = tpls.find((x) => x.def.name === role)
  if (!tp) return t('teamTpl.ownRole')
  const same = tp.def.duty === def.duty && (tp.def.reviewer ?? null) === (def.reviewer ?? null)
    && tp.def.model_slot === def.model_slot && tp.def.skills.join('\n') === def.skills.join('\n')
  const from = t('wizard.roles.fromTemplate', { name: tp.def.name })
  return same ? from : `${from} · ${t('wizard.roles.localCustom')}`
}

/** 项目 Agent 详情（gap D2）：头部「休眠 / 唤醒」，保存行左侧「移出团队」（AlertDialog danger，首焦取消）。
 *  移出是 core 软删并守门（team_removal：在跑 / 有待决 / 快速通道指定人拒绝，原因码走 errors.*）。 */
function AgentDetailPane({ member, tpls, onRemoved, onClose }: {
  member: TeamRow
  tpls: RoleTemplate[] | null
  onRemoved: () => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  const providers = useUiStore((s) => s.providers)
  const avatars = useUiStore((s) => s.avatars)
  const invalidate = useUiStore((s) => s.invalidate)
  const pushToast = useUiStore((s) => s.pushToast)
  const askConfirm = useUiStore((s) => s.askConfirm)
  const [def, setDef] = useState<AgentDetail['def'] | null>(null)
  const sleeping = member.status === 'sleeping'

  useEffect(() => {
    let live = true
    api.agentDetail(member.id).then((d) => { if (live) setDef(d.def) }).catch(() => {})
    return () => { live = false }
  }, [member.id])

  const toggleSleep = async () => {
    try { await api.setAgentSleeping(member.id, !sleeping); await invalidate('team') }
    catch (e) { pushToast(errText(e), 'err') }
  }
  const remove = () =>
    askConfirm({
      title: t('teamTpl.removeTitle', { name: member.role }),
      body: t('teamTpl.removeBody'),
      danger: true,
      confirmLabel: t('teamTpl.remove'),
      run: async () => {
        try {
          await api.removeAgent(member.id)
          pushToast(t('teamTpl.removed', { name: member.role }), 'ok')
          onRemoved()
        } catch (e) { pushToast(errText(e), 'err') }
      },
    })

  return (
    <div data-agent-detail className="max-w-[560px]">
      <DetailHead
        lead={<Avatar agentId={member.id} name={member.role} size={40} src={avatars[member.id]} />}
        title={member.role}
        sub={templateOrigin(member.role, def, tpls, t) ?? slotLabel(member.model_slot, providers, t('agent.dedicatedTag'))}
        actions={
          <Button size="sm" data-agent-sleep onClick={() => void toggleSleep()}>
            <Icon name={sleeping ? 'bolt' : 'sleep'} size={14} />{sleeping ? t('agent.wake') : t('agent.sleep')}
          </Button>
        }
      />
      <div className="mt-4">
        <RoleEditor key={member.id} agentId={member.id} density="md" onClose={onClose}
          footStart={
            <Button size="sm" variant="ghost" className="text-red" data-agent-remove onClick={remove}>
              {t('teamTpl.remove')}
            </Button>
          } />
      </div>
    </div>
  )
}

const blank: RoleDef = { name: '', duty: '', reviewer: null, model_slot: 'chat', globs: [], skills: [] }

/** 角色模板库（ADR 0057 + settings-3col 票 03）：内置 ∪ ~/.hexagon/roles.json。 */
function TemplateLibrary({ head }: { head?: React.ReactNode }) {
  const { t } = useTranslation()
  const { tpls, error, load } = useTemplates()
  const [sel, setSel] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  useEffect(load, [load])

  const names = (tpls ?? []).map((x) => x.def.name)
  const selTpl = tpls?.find((x) => x.def.name === sel) ?? null

  return (
    <ListDetail<RoleTemplate>
      label={t('teamTpl.library')}
      head={head}
      items={tpls ?? []}
      loading={tpls === null}
      error={error ? { text: error, onRetry: load } : undefined}
      itemKey={(x) => x.def.name}
      filterText={(x) => `${x.def.name} ${x.def.duty}`}
      selected={creating ? null : sel}
      onSelect={(k) => { setSel(k); setCreating(false) }}
      searchPlaceholder={t('teamTpl.filter')}
      emptyText={t('teamTpl.empty')}
      foot={
        <Button size="sm" className="flex-1" onClick={() => { setCreating(true); setSel(null) }}>
          <Icon name="plus" size={14} />{t('teamTpl.newTpl')}
        </Button>
      }
      renderItem={(tp) => (
        <>
          <Avatar name={tp.def.name} size={24} />
          <div className="min-w-0 flex-1">
            <div className="truncate">{tp.def.name}</div>
            <div className="truncate text-[12px] text-ink-3">{tp.def.duty}</div>
          </div>
          <Badge variant={tp.origin === 'custom' ? 'info' : 'line'} className="h-[18px] px-1.5 text-[11px]">{t(`teamTpl.${tp.origin}`)}</Badge>
        </>
      )}
    >
      {creating ? (
        <TemplateEditor def={blank} names={names} isCustom onDone={() => { setCreating(false); load() }} />
      ) : selTpl ? (
        <TemplateEditor key={selTpl.def.name} def={selTpl.def} names={names} isCustom={selTpl.origin === 'custom'}
          onDone={() => { setSel(null); load() }} />
      ) : (
        <Empty icon="users" title={t('teamTpl.library')} desc={t('teamTpl.unselected')} />
      )}
    </ListDetail>
  )
}

/** 项目 Agent 清单 | RoleEditor；「从模板添加」「创建角色」在底部操作行。 */
function ProjectTeam({ head }: { head: React.ReactNode }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const providers = useUiStore((s) => s.providers)
  const avatars = useUiStore((s) => s.avatars)
  const refreshSlow = useUiStore((s) => s.refreshSlow)
  const pushToast = useUiStore((s) => s.pushToast)
  const [sel, setSel] = useState<string | null>(null)
  const [mode, setMode] = useState<'none' | 'addTpl' | 'create' | 'editTpl'>('none')
  const { tpls, load } = useTemplates()
  const [editTpl, setEditTpl] = useState<RoleTemplate | null>(null)

  // Launcher 进设置时 team 可能是空的——分区挂载即补拉慢切片。
  useEffect(() => { void refreshSlow(['team']) }, [refreshSlow])
  // 详情副行「来自模板…」也要模板库（gap D2），选中 Agent 时同样补拉。
  useEffect(() => { if ((mode === 'addTpl' || sel) && tpls === null) load() }, [mode, sel, tpls, load])

  const selAgent = team.find((m) => m.id === sel) ?? null
  const addFromTpl = async (tp: RoleTemplate) => {
    try {
      await api.createRole(tp.def)
      pushToast(t('teamTpl.added', { name: tp.def.name }), 'ok')
      setMode('none')
      await refreshSlow(['team'])
    } catch (e) { pushToast(errText(e), 'err') }
  }

  return (
    <ListDetail
      label={t('settings.nav_team')}
      head={head}
      items={team}
      itemKey={(m) => m.id}
      filterText={(m) => m.role}
      itemDim={(m) => m.status === 'sleeping'}
      selected={sel}
      onSelect={(k) => { setSel(k); setMode('none'); setEditTpl(null) }}
      searchPlaceholder={t('teamTpl.filterAgents')}
      emptyText={t('side.noMembers')}
      foot={
        <>
          <Button size="sm" className="flex-1" onClick={() => { setMode('addTpl'); setSel(null); setEditTpl(null) }}>
            <Icon name="plus" size={14} />{t('teamTpl.fromTpl')}
          </Button>
          <Button size="sm" className="flex-1" onClick={() => { setMode('create'); setSel(null); setEditTpl(null) }}>
            {t('agent.createRole')}
          </Button>
        </>
      }
      renderItem={(m) => (
        <>
          <Avatar agentId={m.id} name={m.role} size={24} src={avatars[m.id]} status={m.status === 'sleeping' ? 'sleep' : 'online'} />
          <div className="min-w-0 flex-1">
            <div className="truncate">{m.role}</div>
            <div className="truncate text-[12px] text-ink-3">
              {m.status === 'sleeping' ? t('sidebar.sleeping') : slotLabel(m.model_slot, providers, t('agent.dedicatedTag'))}
            </div>
          </div>
        </>
      )}
    >
      {mode === 'create' ? (
        <div className="max-w-[520px]">
          <CreateRoleForm density="md" onDone={() => { setMode('none'); void refreshSlow(['team']) }} />
        </div>
      ) : editTpl ? (
        <TemplateEditor key={editTpl.def.name} def={editTpl.def} names={(tpls ?? []).map((x) => x.def.name)}
          isCustom={editTpl.origin === 'custom'} onDone={() => { setEditTpl(null); load() }} />
      ) : mode === 'addTpl' ? (
        <div data-template-picker className="max-w-[560px]">
          <DetailHead title={t('teamTpl.fromTpl')} sub={t('teamTpl.pickTpl')} />
          <div className="mt-4 rounded-[var(--r-card)] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
            {(tpls ?? []).map((tp) => (
              <div key={tp.def.name} className="flex items-center gap-2.5 border-t border-line-soft px-3.5 py-2.5 first:border-t-0">
                <Avatar name={tp.def.name} size={24} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5 text-[13px] font-medium text-ink">
                    {tp.def.name}
                    <Badge variant={tp.origin === 'custom' ? 'info' : 'line'} className="h-[18px] px-1.5 text-[11px]">{t(`teamTpl.${tp.origin}`)}</Badge>
                  </div>
                  <div className="truncate text-[12px] text-ink-3">{tp.def.duty}</div>
                </div>
                <Button size="xs" variant="ghost" onClick={() => setEditTpl(tp)}>{t('mcp.edit')}</Button>
                <Button size="xs" variant="primary" onClick={() => void addFromTpl(tp)}>{t('teamTpl.add')}</Button>
              </div>
            ))}
            {tpls !== null && tpls.length === 0 && <Empty icon="users" title={t('teamTpl.empty')} />}
          </div>
        </div>
      ) : selAgent ? (
        <AgentDetailPane member={selAgent} tpls={tpls}
          onClose={() => setSel(null)}
          onRemoved={() => { setSel(null); void refreshSlow(['team']) }} />
      ) : (
        <Empty icon="users" title={t('settings.nav_team')} desc={t('teamTpl.unselectedAgent')} />
      )}
    </ListDetail>
  )
}

export function TeamSection({ projectless = false }: { projectless?: boolean }) {
  const { t } = useTranslation()
  const [view, setView] = useState<'project' | 'library'>('project')
  // projectless 无项目态：不发项目 IPC（team 拉取必败），只渲染模板库。
  if (projectless) return <TemplateLibrary />
  const head = (
    <Segmented aria-label={t('settings.nav_team')} className="mx-3 mt-3" value={view} onValueChange={(v) => setView(v as 'project' | 'library')}>
      <SegmentedItem value="project">{t('settingsUi.teamProject')}</SegmentedItem>
      <SegmentedItem value="library">{t('teamTpl.library')}</SegmentedItem>
    </Segmented>
  )
  return view === 'project' ? <ProjectTeam head={head} /> : <TemplateLibrary head={head} />
}
