// 角色详情 RoleDetail（components E4，entry#wz-roles；设置团队与向导共用的形制）。
// 本文件是向导形：受控组件，每次字段变化都经 onChange 交给调用方——ADR 0091
// 「定制即改即生效」，没有保存 / 放弃；撤销入口是头部「恢复模板」。
// 改出的 RoleDef 只进本项目（向导草稿），不回写模板库（ADR 0057）。
// ADR 0075：归属 globs 收进「高级选项」折叠，留空 = 不限制写入范围。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { ProvidersView, RoleDef } from '../api'
import { sharedSlots, slotLabel } from '../modelpick'
import { Avatar } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Field, Textarea } from '@/components/ui/input'
import { NativeSelect } from '@/components/ui/native-select'
import { EntityChips } from './EntityPicker'
import { Icon } from './Icon'
import { CardLoading } from './wizard/CardLoading'

export function RoleDetail({ def, tplName, names, doc, customized, aiDrafted, edited, busy, onChange, onRestore }: {
  def: RoleDef
  /** 来源模板名（向导里与 def.name 相同）。 */
  tplName: string
  /** 可选上级名单（含自己，渲染时排除）。 */
  names: string[]
  doc: ProvidersView
  customized: boolean
  aiDrafted: boolean
  /** 本次进入后发生过编辑：头部换成「改动已存入向导草稿」确认行。 */
  edited: boolean
  /** AI 起草在途：职责框换像素格，其余字段只读（E4 禁用 / 加载）。 */
  busy: boolean
  onChange: (def: RoleDef) => void
  onRestore: () => void
}) {
  const { t } = useTranslation()
  const [adv, setAdv] = useState(false)
  const upd = (patch: Partial<RoleDef>) => onChange({ ...def, ...patch })
  return (
    <div data-role-customize aria-label={def.name} className="flex flex-col gap-3">
      <div className="flex items-center gap-2.5">
        <Avatar name={def.name} size={30} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <b className="truncate text-[14px] font-semibold">{def.name}</b>
            {customized && !aiDrafted && <Badge className="h-[18px] text-[11px]">{t('wizard.customized')}</Badge>}
            {aiDrafted && <Badge variant="info" className="h-[18px] text-[11px]">{t('wizard.roles.aiChip')}</Badge>}
          </div>
          {edited ? (
            <div role="status" className="flex items-center gap-1 text-[12px] text-green">
              <Icon name="check" size={12} />{t('wizard.savedToDraft')}
            </div>
          ) : (
            <div className="truncate text-[12px] text-ink-3">
              {t('wizard.roles.fromTemplate', { name: tplName })}
              {customized ? ` · ${t('wizard.roles.localCustom')}` : ''}
            </div>
          )}
        </div>
        <Button size="xs" variant="ghost" disabled={!customized && !edited} onClick={onRestore} data-restore-template>
          <Icon name="undo" />{t('wizard.resetTpl')}
        </Button>
      </div>
      <Field label={t('agent.duty')}>
        {busy ? (
          <CardLoading label={t('wizard.draftingRoles')} className="min-h-[88px]" />
        ) : (
          <Textarea aria-label={t('agent.duty')} value={def.duty} onChange={(e) => upd({ duty: e.target.value })} />
        )}
      </Field>
      <Field label={t('agent.reviewer')}>
        <NativeSelect aria-label={t('agent.reviewer')} disabled={busy} value={def.reviewer ?? ''} onChange={(e) => upd({ reviewer: e.target.value || null })}>
          <option value="">{t('agent.noReviewer')}</option>
          {names.filter((n) => n !== def.name).map((n) => <option key={n} value={n}>{n}</option>)}
          {/* 模板指向的上级不在本次模板目录里时也要显示，否则下拉显示成「直达负责人」而实际值不是 */}
          {def.reviewer && !names.includes(def.reviewer) && <option value={def.reviewer}>{def.reviewer}</option>}
        </NativeSelect>
      </Field>
      <Field label={t('agent.modelSlot')}>
        <NativeSelect aria-label={t('agent.modelSlot')} disabled={busy} value={def.model_slot} onChange={(e) => upd({ model_slot: e.target.value })}>
          {sharedSlots(doc.slots, def.model_slot).map((s) => (
            <option key={s} value={s}>{slotLabel(s, doc, t('agent.dedicatedTag'))}</option>
          ))}
        </NativeSelect>
      </Field>
      <div className="flex flex-col">
        <span className="mb-1.5 text-[12.5px] font-medium">{t('wizard.roles.skills')}</span>
        <EntityChips value={def.skills} onChange={(ids) => upd({ skills: ids })} source="skills" />
      </div>
      <AdvancedGlobs open={adv} onToggle={() => setAdv((v) => !v)} value={def.globs} disabled={busy} onChange={(globs) => upd({ globs })} />
    </div>
  )
}

/// 「高级选项（写入范围，留空 = 不限制）」折叠（ADR 0075）。新建角色表单同款。
export function AdvancedGlobs({ open, onToggle, value, disabled, onChange }: {
  open: boolean
  onToggle: () => void
  value: string[]
  disabled?: boolean
  onChange: (globs: string[]) => void
}) {
  const { t } = useTranslation()
  // 本地保留原文：逐键解析会吞掉刚敲的换行；外部改值（恢复模板）时原文与解析不一致就以外部为准
  const [text, setText] = useState(value.join('\n'))
  const parse = (s: string) => s.split('\n').map((x) => x.trim()).filter(Boolean)
  const shown = parse(text).join('\n') === value.join('\n') ? text : value.join('\n')
  return (
    <div className="flex flex-col gap-1.5">
      <button
        type="button"
        data-globs-toggle
        aria-expanded={open}
        onClick={onToggle}
        className="hx-press hx-focus inline-flex cursor-pointer items-center gap-1.5 self-start rounded-[6px] text-[13px] text-ink-2"
      >
        <Icon name={open ? 'chevron-down' : 'chevron-right'} size={12} />
        {t('agent.advanced')}（{t('agent.globsEmpty')}）
      </button>
      {open && (
        <Textarea
          aria-label={t('agent.globs')}
          placeholder="src/**"
          disabled={disabled}
          className="font-mono text-[12px]"
          value={shown}
          onChange={(e) => { setText(e.target.value); onChange(parse(e.target.value)) }}
        />
      )}
    </div>
  )
}
