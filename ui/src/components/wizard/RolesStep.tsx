// ④ 团队角色步（interaction §11，entry#wz-roles，components E2 / E4 / E10，ADR 0091）：
// 卡内「列表 | 详情」。勾选框只决定要不要这个角色，点整行才是定制（避免误把定制当成取消勾选）。
// 定制即改即生效写入向导草稿；撤销是详情头部「恢复模板」；没有保存 / 放弃。
// 新建角色是例外：它写入全局模板库 ~/.hexagon/roles.json，表单末尾保留显式「创建角色」（次要样式）。
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ProvidersView, type RoleDef, type RoleTemplate } from '../../api'
import { cn } from '@/lib/utils'
import { Avatar } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Field, Input, Textarea } from '@/components/ui/input'
import { NativeSelect } from '@/components/ui/native-select'
import { EntityChips } from '../EntityPicker'
import { Icon } from '../Icon'
import { AdvancedGlobs, RoleDetail } from '../RoleDetail'
import { CardLoading } from './CardLoading'
import { sharedSlots, slotLabel } from '../../modelpick'
import type { Draft } from './draft'

export function RolesStep(p: {
  tpls: RoleTemplate[]
  draft: Draft
  effDef: (tp: RoleTemplate) => RoleDef
  aiDraftedNames: Set<string>
  selected: string | null
  onSelect: (name: string) => void
  onToggle: (name: string, on: boolean) => void
  onSelectAll: () => void
  makingRole: boolean
  onNewRole: () => void
  edited: Set<string>
  onEdit: (tp: RoleTemplate, def: RoleDef) => void
  onRestore: (name: string) => void
  rolesBusy: boolean
  doc: ProvidersView
  aiDraft: { label: string; title: string; disabled: boolean; onClick: () => void }
  newRole: { onDone: (def: RoleDef) => void; onCancel: () => void; onBusy: (b: boolean) => void }
  footer: React.ReactNode
}) {
  const { t } = useTranslation()
  const { tpls, draft } = p
  const names = tpls.map((tp) => tp.def.name)
  const sel = tpls.find((tp) => tp.def.name === p.selected)
  const listRef = useRef<HTMLDivElement>(null)

  const move = (to: number) => {
    const tp = tpls[Math.max(0, Math.min(tpls.length - 1, to))]
    if (!tp) return
    p.onSelect(tp.def.name)
    listRef.current?.querySelector<HTMLElement>(`[data-role-row="${CSS.escape(tp.def.name)}"]`)?.focus()
  }
  const onListKey = (e: React.KeyboardEvent) => {
    const i = tpls.findIndex((tp) => tp.def.name === p.selected)
    const to = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: tpls.length - 1 }[e.key]
    if (to === undefined || e.altKey || e.metaKey || e.ctrlKey) return
    e.preventDefault()
    move(to)
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-3 border-b border-line-soft px-[22px] pt-5 pb-3">
        <div className="min-w-0 flex-1">
          <h3 tabIndex={-1} className="text-[15px] font-semibold outline-none">{t('wizard.heading.roles')}</h3>
          <div className="text-[12px] text-ink-3">{t('wizard.lead.roles')}</div>
        </div>
        <Button size="sm" data-ai-draft disabled={p.aiDraft.disabled} title={p.aiDraft.title} loading={p.rolesBusy} onClick={p.aiDraft.onClick}>
          {!p.rolesBusy && <Icon name="sparkle" size={14} />}{p.aiDraft.label}
        </Button>
      </div>
      <div className="flex min-h-0 flex-1">
        <div className="flex w-[260px] flex-none flex-col border-r border-line-soft">
          <div className="flex items-center gap-1 pt-2.5 pr-2.5 pb-1 pl-[18px]">
            <span className="flex-1 text-[12px] text-ink-3">
              {t('wizard.roles.listHead', { picked: draft.roles.filter((n) => names.includes(n)).length, total: tpls.length })}
            </span>
            <Button size="xs" variant="ghost" onClick={p.onSelectAll}>{t('wizard.roles.selectAll')}</Button>
            <Button size="xs" variant="ghost" data-new-role aria-pressed={p.makingRole} onClick={p.onNewRole}>
              <Icon name="plus" size={14} />{t('wizard.roles.new')}
            </Button>
          </div>
          <div
            ref={listRef}
            role="listbox"
            aria-label={t('wizard.rail.roles')}
            onKeyDown={onListKey}
            className="min-h-0 flex-1 overflow-auto pb-2"
          >
            {tpls.map((tp) => {
              const name = tp.def.name
              const picked = draft.roles.includes(name)
              const isSel = !p.makingRole && p.selected === name
              const customized = !!draft.roleOverrides[name]
              const ai = p.aiDraftedNames.has(name)
              return (
                <div
                  key={name}
                  role="option"
                  aria-selected={isSel}
                  tabIndex={isSel || (!p.selected && tp === tpls[0]) ? 0 : -1}
                  data-role-row={name}
                  onClick={() => p.onSelect(name)}
                  className={cn(
                    'hx-focus mx-2 my-px flex cursor-pointer items-start gap-2.5 rounded-[var(--r-card)] p-2.5 outline-none',
                    isSel ? 'bg-accent-tint shadow-[inset_2px_0_0_var(--accent)]' : 'hover:bg-hover',
                    !picked && 'opacity-60',
                  )}
                >
                  <span className="pt-0.5" onClick={(e) => e.stopPropagation()}>
                    <Checkbox
                      aria-label={name}
                      checked={picked}
                      onCheckedChange={(v) => p.onToggle(name, v === true)}
                    />
                  </span>
                  <Avatar name={name} size={24} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5">
                      <b className="truncate text-[13px] font-medium">{name}</b>
                      {ai && <Badge variant="info" className="h-[18px] text-[11px]">{t('wizard.roles.aiChip')}</Badge>}
                      {customized && !ai && <Badge className="h-[18px] text-[11px]">{t('wizard.customized')}</Badge>}
                      {tp.origin === 'custom' && <Badge variant="line" className="h-[18px] text-[11px]">{t('teamTpl.custom')}</Badge>}
                    </div>
                    <div className="truncate text-[12px] text-ink-3" title={p.effDef(tp).duty}>{p.effDef(tp).duty}</div>
                  </div>
                  {isSel && <Icon name="chevron-right" size={12} className="mt-[3px] text-ink-3" />}
                </div>
              )
            })}
          </div>
        </div>
        <div className="min-w-0 flex-1 overflow-auto px-5 py-4">
          {p.makingRole ? (
            <NewRoleForm names={names} doc={p.doc} {...p.newRole} />
          ) : sel ? (
            <RoleDetail
              key={sel.def.name}
              def={p.effDef(sel)}
              tplName={sel.def.name}
              names={names}
              doc={p.doc}
              customized={!!draft.roleOverrides[sel.def.name]}
              aiDrafted={p.aiDraftedNames.has(sel.def.name)}
              edited={p.edited.has(sel.def.name)}
              busy={p.rolesBusy}
              onChange={(def) => p.onEdit(sel, def)}
              onRestore={() => p.onRestore(sel.def.name)}
            />
          ) : (
            <div className="pt-10 text-center text-[13px] text-ink-3">{t('wizard.roles.pickHint')}</div>
          )}
          {p.footer}
        </div>
      </div>
    </div>
  )
}

/** 新建角色（详情区，与定制互斥）。写入全局模板库，所以必须显式点「创建角色」（ADR 0091 例外条）；
 *  取消整份丢弃。「AI 起草」是模型调用：onBusy 上报父级锁跳步。 */
function NewRoleForm({ names, doc, onDone, onCancel, onBusy }: {
  names: string[]
  doc: ProvidersView
  onDone: (def: RoleDef) => void
  onCancel: () => void
  onBusy: (b: boolean) => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [duty, setDuty] = useState('')
  const [reviewer, setReviewer] = useState('')
  const [slot, setSlot] = useState('default')
  const [globs, setGlobs] = useState<string[]>([])
  const [skills, setSkills] = useState<string[]>([])
  const [err, setErr] = useState('')
  const [drafting, setDrafting] = useState(false)
  const [saving, setSaving] = useState(false)
  const [adv, setAdv] = useState(false)

  const draftDuty = () => {
    setDrafting(true)
    onBusy(true)
    api.draftRoleDuty(name.trim(), duty || ' ')
      .then((text) => setDuty(text.trim()))
      .catch((e) => setErr(errText(e)))
      .finally(() => {
        setDrafting(false)
        onBusy(false)
      })
  }
  const create = () => {
    const def: RoleDef = {
      name: name.trim(),
      duty: duty.trim(),
      reviewer: reviewer || null,
      model_slot: slot.trim() || 'default',
      globs,
      skills,
    }
    setSaving(true)
    setErr('')
    api.saveRoleTemplate(def).then(() => onDone(def)).catch((e) => setErr(errText(e))).finally(() => setSaving(false))
  }
  return (
    <div data-new-role-form aria-label={t('agent.createRole')} className="flex flex-col gap-3">
      <div className="flex items-center gap-2.5">
        <span className="grid size-[30px] place-items-center rounded-[9px] bg-canvas text-ink-2 shadow-[inset_0_0_0_1px_var(--line)]">
          <Icon name="plus" size={14} />
        </span>
        <div className="min-w-0 flex-1">
          <b className="text-[14px] font-semibold">{t('agent.createRole')}</b>
          <div className="text-[12px] text-ink-3">{t('wizard.roles.newHint')}</div>
        </div>
      </div>
      <Field label={t('agent.roleName')}>
        <Input aria-label={t('agent.roleName')} value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label={t('agent.duty')}>
        {drafting ? (
          <CardLoading label={t('wizard.draftingRoles')} className="min-h-[88px]" />
        ) : (
          <Textarea aria-label={t('agent.duty')} value={duty} onChange={(e) => setDuty(e.target.value)} />
        )}
      </Field>
      <Button size="xs" className="-mt-1 self-start" disabled={!name.trim() || drafting} onClick={draftDuty}>
        <Icon name="sparkle" size={14} />{t('agent.draftAi')}
      </Button>
      <Field label={t('agent.reviewer')}>
        <NativeSelect aria-label={t('agent.reviewer')} value={reviewer} onChange={(e) => setReviewer(e.target.value)}>
          <option value="">{t('agent.noReviewer')}</option>
          {names.map((n) => <option key={n} value={n}>{n}</option>)}
        </NativeSelect>
      </Field>
      <Field label={t('agent.modelSlot')}>
        <NativeSelect aria-label={t('agent.modelSlot')} value={slot} onChange={(e) => setSlot(e.target.value)}>
          {sharedSlots(doc.slots, slot).map((s) => <option key={s} value={s}>{slotLabel(s, doc, t('agent.dedicatedTag'))}</option>)}
        </NativeSelect>
      </Field>
      <div className="flex flex-col">
        <span className="mb-1.5 text-[12.5px] font-medium">{t('wizard.roles.skills')}</span>
        <EntityChips value={skills} onChange={setSkills} source="skills" />
      </div>
      <AdvancedGlobs open={adv} onToggle={() => setAdv((v) => !v)} value={globs} onChange={setGlobs} />
      {err && <div role="alert" className="text-[12px] text-red-ink">{err}</div>}
      <div className="flex items-center justify-end gap-2 pt-1">
        <Button size="sm" variant="ghost" onClick={onCancel}>{t('agent.cancel')}</Button>
        <Button size="sm" data-create-role disabled={!name.trim() || drafting} loading={saving} onClick={create}>
          {t('wizard.roles.create')}
        </Button>
      </div>
    </div>
  )
}
