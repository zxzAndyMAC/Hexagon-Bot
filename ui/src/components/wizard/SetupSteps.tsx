// ②项目目录、⑥密钥检查、⑦确认创建（原型无画面，components E9 补设计：A9 Field + A4 胶囊 + A11 复选 + A8 inset 块）。
// 行为不变：ADR 0060 脏树不停步；已有工作台状态只能打开；密钥步全槽就绪才放行；确认才写盘。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, isTauri, type DirReport, type ProviderView, type ProvidersView, type RoleDef, type RoleTemplate } from '../../api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Field, Input } from '@/components/ui/input'
import { IconButton } from '@/components/ui/icon-button'
import { NativeSelect } from '@/components/ui/native-select'
import { Icon } from '../Icon'
import type { Draft } from './draft'

export function DirStep({ draft, set, report, busy, err, onBrowse, onOpenExisting }: {
  draft: Draft
  set: (patch: Partial<Draft>) => void
  report: DirReport | null
  busy: boolean
  err: string
  onBrowse: () => void
  onOpenExisting: () => void
}) {
  const { t } = useTranslation()
  return (
    <div className="flex max-w-[560px] flex-col gap-3">
      <Field label={t('wizard.dirLabel')}>
        <div className="flex gap-2">
          <Input aria-label={t('wizard.dirLabel')} value={draft.dir} placeholder={t('wizard.dirPlaceholder')} onChange={(e) => set({ dir: e.target.value })} />
          {isTauri && <Button size="sm" className="h-[34px]" onClick={onBrowse}><Icon name="folder" />{t('wizard.browse')}</Button>}
        </div>
      </Field>
      <Field label={t('wizard.nameLabel')}>
        <Input aria-label={t('wizard.nameLabel')} value={draft.name} onChange={(e) => set({ name: e.target.value })} />
      </Field>
      {report && (
        <div className="flex flex-wrap gap-1.5">
          <Badge variant={report.is_git ? 'ok' : 'neutral'}>{report.is_git ? t('wizard.isGit') : t('wizard.noGit')}</Badge>
          {report.empty && <Badge>{t('wizard.emptyDir')}</Badge>}
          {report.dirty && <Badge variant="warn">{t('wizard.dirty')}</Badge>}
          {report.has_workbench && <Badge variant="info">{t('wizard.hasWorkbench')}</Badge>}
          {report.instructions && <Badge variant="ok">{report.instructions}</Badge>}
        </div>
      )}
      {report?.dirty && <div className="text-[12.5px] text-ink-2">{t('wizard.dirtyHint')}</div>}
      {report?.has_workbench && (
        <div className="flex items-center gap-3 rounded-[10px] bg-inset px-3 py-2.5 text-[12.5px] shadow-[inset_0_0_0_1px_var(--line-soft)]">
          <span className="flex-1">{t('wizard.openInstead')}</span>
          <Button size="sm" variant="ink" loading={busy} onClick={onOpenExisting}>{t('wizard.openProject')}</Button>
        </div>
      )}
      {err && <div role="alert" className="text-[12.5px] text-red">{err}</div>}
      {report && !report.is_git && !report.has_workbench && (
        <label className="flex items-center gap-2 text-[13px]">
          <Checkbox data-init-git checked={draft.initGit} onCheckedChange={(v) => set({ initGit: v === true })} />
          {t('wizard.initGit')}
        </label>
      )}
    </div>
  )
}

export function KeysStep({ slots, doc, pickedRoles, effDef, busy, onSaveKey }: {
  slots: string[]
  doc: ProvidersView
  pickedRoles: RoleTemplate[]
  effDef: (tp: RoleTemplate) => RoleDef
  busy: boolean
  onSaveKey: (provider: ProviderView, secret: string) => Promise<boolean>
}) {
  const { t } = useTranslation()
  const bindingOf = (slot: string) => {
    const b = doc.slots[slot] ?? doc.slots.default
    if (!b) return null
    const provider = doc.providers.find((p) => p.id === b.provider_id)
    return provider ? { binding: b, provider } : null
  }
  const unready = slots.filter((s) => {
    const r = bindingOf(s)
    return !(r && r.provider.enabled && r.provider.key_set)
  })
  return (
    <div className="flex flex-col gap-2">
      {slots.map((slot) => {
        const r = bindingOf(slot)
        const ready = !!(r && r.provider.enabled && r.provider.key_set)
        const viaDefault = !doc.slots[slot] && !!doc.slots.default
        return (
          <div key={slot} data-key-slot={slot} className="flex flex-col gap-2 rounded-[10px] bg-inset px-3 py-2.5 shadow-[inset_0_0_0_1px_var(--line-soft)]">
            <div className="flex items-center gap-2">
              <div className="min-w-0 flex-1">
                <code className="font-mono text-[12px]">model/{slot}</code>
                <div className="truncate text-[12px] text-ink-3">
                  {pickedRoles.filter((tp) => effDef(tp).model_slot === slot).map((tp) => tp.def.name).join('、')}
                  {r && ` · ${r.provider.name} · ${r.binding.model}`}
                  {r && viaDefault && ` · ${t('wizard.viaDefault')}`}
                </div>
              </div>
              {ready && <Badge variant="ok"><Icon name="check" />{t('wizard.keyOk')}</Badge>}
              {r && !r.provider.enabled && <Badge variant="warn">{t('providers.disabledTag')}</Badge>}
            </div>
            {r && r.provider.enabled && !r.provider.key_set && <KeyRow provider={r.provider} busy={busy} onSave={onSaveKey} />}
            {!r && <div className="text-[12px] text-ink-3">{t('wizard.noProvider', { slot })}</div>}
          </div>
        )
      })}
      {unready.length > 0 && (
        <div className="mt-1 flex items-center gap-2.5">
          <Button size="sm" onClick={() => api.openSettings().catch(() => {})}><Icon name="settings" />{t('wizard.goSettings')}</Button>
          <span className="text-[12.5px] text-orange">{t('wizard.keysBlocked')}</span>
        </div>
      )}
    </div>
  )
}

function KeyRow({ provider, busy, onSave }: {
  provider: ProviderView
  busy: boolean
  onSave: (provider: ProviderView, secret: string) => Promise<boolean>
}) {
  const { t } = useTranslation()
  const [secret, setSecret] = useState('')
  const [shown, setShown] = useState(false)
  return (
    <div className="flex items-center gap-2">
      <Input
        type={shown ? 'text' : 'password'}
        aria-label={t('providers.key')}
        placeholder={t('wizard.keyPlaceholder')}
        value={secret}
        onChange={(e) => setSecret(e.target.value)}
        trailing={
          <IconButton size="sm" label={t(shown ? 'providers.hideKey' : 'providers.showKey')} onClick={() => setShown((s) => !s)}>
            <Icon name={shown ? 'vision' : 'vision-off'} />
          </IconButton>
        }
      />
      <Button
        size="sm"
        className="h-[34px]"
        disabled={!secret.trim()}
        loading={busy}
        onClick={() => { void onSave(provider, secret.trim()).then((ok) => { if (ok) setSecret('') }) }}
      >
        {t('wizard.keySave')}
      </Button>
    </div>
  )
}

export function ConfirmSummary({ draft, set, pickedRoles, approvedQuality, report, existingRepoAlign }: {
  draft: Draft
  set: (patch: Partial<Draft>) => void
  pickedRoles: RoleTemplate[]
  approvedQuality: Record<string, string>
  report: DirReport | null
  existingRepoAlign: boolean
}) {
  const { t } = useTranslation()
  const row = (label: string, value: React.ReactNode) => (
    <div className="grid grid-cols-[96px_1fr] gap-3 py-1.5 text-[13px]">
      <span className="text-ink-3">{label}</span>
      <span className="min-w-0 break-words">{value}</span>
    </div>
  )
  return (
    <div className="flex flex-col">
      <div className="divide-y divide-line-soft">
        {row(t('wizard.sumDir'), <code className="font-mono text-[12px]">{draft.dir}</code>)}
        {row(t('wizard.sumName'), draft.name)}
        {row(t('wizard.sumRoles'), (
          <>
            {draft.roles.map((n) => (draft.roleOverrides[n] ? `${n}*` : n)).join('、')}
            {Object.keys(draft.roleOverrides).some((n) => draft.roles.includes(n)) && (
              <span className="ml-1.5 text-[12px] text-ink-3">{t('wizard.customMark')}</span>
            )}
          </>
        ))}
        {row(t('wizard.sumMode'), draft.fastPath ? `${t('wizard.fastMode')} · ${draft.fastRole}` : (draft.flowPack?.name || t('wizard.flowDraft')))}
        {row(t('wizard.sumAgents'), report?.instructions
          ? report.instructions
          : draft.agentsMd.trim() && (report?.empty || draft.genAgents) ? t('wizard.sumAgentsGen') : t('wizard.sumAgentsNone'))}
      </div>
      <label className="mt-3 flex items-center gap-2 text-[13px]">
        <Checkbox
          data-fast-path
          checked={draft.fastPath}
          onCheckedChange={(v) => set({ fastPath: v === true, mode: v === true ? 'fastpath' : 'pack' })}
        />
        {t('wizard.fastOnConfirm')}
      </label>
      {draft.fastPath && (
        <NativeSelect aria-label={t('wizard.fastPick')} className="mt-2 w-auto min-w-[200px] self-start" value={draft.fastRole} onChange={(e) => set({ fastRole: e.target.value })}>
          <option value="">{t('wizard.fastPick')}</option>
          {pickedRoles.map((tp) => <option key={tp.def.name} value={tp.def.name}>{tp.def.name}</option>)}
        </NativeSelect>
      )}
      {!draft.fastPath && (
        <div className="mt-3 rounded-[10px] bg-inset px-3 py-2.5 text-[12.5px] shadow-[inset_0_0_0_1px_var(--line-soft)]">
          <b className="font-medium">{t('quality.setupTitle')}</b>
          <p className="mt-0.5 text-ink-3">{t('quality.setupReview')}</p>
          {Object.entries(approvedQuality).length === 0 ? <p className="mt-1">{t('quality.setupNone')}</p> :
            Object.entries(approvedQuality).map(([category, command]) => (
              <div key={category} className="mt-1">
                {t(`quality.category.${category}`)}: <code className="font-mono break-all whitespace-pre-wrap">{command}</code>
              </div>
            ))}
        </div>
      )}
      {existingRepoAlign && (
        <div className="mt-3 rounded-[10px] bg-violet-tint px-3 py-2.5 text-[12.5px] text-violet">{t('wizard.alignHint')}</div>
      )}
    </div>
  )
}
