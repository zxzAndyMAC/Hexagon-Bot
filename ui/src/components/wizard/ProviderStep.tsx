// ① 模型服务商步（票 13，entry#wz-provider）：数据只走 list/save/set_slot_binding，不另起一份供应商存储。
// 放行 = default 槽指向的供应商已启用且钥匙已存、模型名非空；已就绪不渲染钥匙框，缺钥匙才要输入。
// gap B8（2026-10-15）：按原型收成三字段「服务商 ▾ / API Key / 默认模型」。下拉 = 已配置供应商 + 未配置的内置预设
// （core provider_presets）+ 自定义；选预设自动填名称、类型、base URL，只有「自定义」才展开这三个框。
// 票 10 原先空表时五个框（名称/类型/地址/钥匙/模型）、有供应商时只能在已有里选，两条路径现合为一个表单。
// gap B4（2026-10-15）：默认模型框实时检测钥匙（probe_provider_key，输入停 600ms 后请求一次模型目录）：
// 检测中 / 检测通过 · 窗口 / 目录里没有这个模型 / 检测失败 + 原因。检测是建议不是闸——保存仍可点。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ProviderDef, type ProviderPreset, type ProviderView, type ProvidersView } from '../../api'
import { useUiStore } from '../../store'
import { providerStepReady, wizardPreset } from '../../providerGate'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { Field, Input } from '@/components/ui/input'
import { NativeSelect } from '@/components/ui/native-select'
import { Avatar } from '@/components/ui/avatar'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Icon } from '../Icon'
import { fmtWindow, useKeyProbe, type ProbeState } from '../../keyProbe'

// 与 ProviderManager 同一套 slug，避免两个界面对同一名字写出不同 id。
const providerSlug = (s: string) =>
  s.trim().toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, '-').replace(/^-+|-+$/g, '') || `p${Date.now()}`

function suggestedModel(doc: ProvidersView, p?: ProviderView): string {
  if (!p) return ''
  const bound = doc.slots.default
  if (bound?.provider_id === p.id && bound.model.trim()) return bound.model
  return p.models[0]?.id ?? ''
}

function initialProviderId(doc: ProvidersView): string {
  const bound = doc.slots.default?.provider_id
  if (bound && doc.providers.some((p) => p.id === bound)) return bound
  return doc.providers.find((p) => p.enabled && p.key_set)?.id ?? doc.providers[0]?.id ?? ''
}

function KeyInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useTranslation()
  const [shown, setShown] = useState(false)
  return (
    <Field label={t('providers.key')} help={t('wizard.keyHelp')}>
      <Input
        type={shown ? 'text' : 'password'}
        aria-label={t('providers.key')}
        placeholder={t('wizard.keyPlaceholder')}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        trailing={
          <IconButton size="sm" label={t(shown ? 'providers.hideKey' : 'providers.showKey')} onClick={() => setShown((s) => !s)}>
            <Icon name={shown ? 'vision' : 'vision-off'} size={14} />
          </IconButton>
        }
      />
    </Field>
  )
}

/** 默认模型框 + 检测结果（后置胶囊 + 下方原因）。 */
function ModelProbeField({ value, onChange, probe, list }: {
  value: string
  onChange: (v: string) => void
  probe: ProbeState
  list?: string
}) {
  const { t } = useTranslation()
  const result = probe.phase === 'done' ? probe.result : null
  const failed = probe.phase === 'error' || (result !== null && !result.ok)
  const reason = probe.phase === 'error'
    ? t('wizard.probe.failedWith', { reason: probe.message })
    : result && !result.ok
      ? t(`wizard.probe.reason.${result.failure ?? 'invalid_response'}`, { defaultValue: t('wizard.probe.reason.invalid_response') })
        + (result.status ? ` (HTTP ${result.status})` : '')
      : undefined
  const unlisted = result?.ok && result.model_listed === false
  const chip = result?.ok
    ? unlisted
      ? <Badge variant="warn" data-probe="unlisted">{t('wizard.probe.unlisted')}</Badge>
      : <Badge variant="ok" data-probe="ok"><Icon name="check" />{result.window ? t('wizard.probe.okWindow', { window: fmtWindow(result.window) }) : t('wizard.probe.ok')}</Badge>
    : failed
      ? <Badge variant="err" data-probe="failed">{t('wizard.probe.failed')}</Badge>
      : undefined
  return (
    <Field label={t('wizard.providerField.model')} error={reason} help={unlisted ? t('wizard.probe.unlistedHelp') : undefined}>
      <Input aria-label={t('wizard.providerField.model')} placeholder={t('providers.pickModel')} value={value} list={list}
        aria-invalid={failed || undefined}
        loadingLabel={probe.phase === 'running' ? t('wizard.probe.running') : undefined}
        trailing={chip}
        onChange={(e) => onChange(e.target.value)} />
    </Field>
  )
}

export function ProviderFirstStep({ doc, loaded, onRefresh }: {
  doc: ProvidersView
  loaded: boolean
  onRefresh: () => void
}) {
  const { t } = useTranslation()
  const ready = loaded && providerStepReady(doc)
  const binding = doc.slots.default
  const bound = binding ? doc.providers.find((p) => p.id === binding.provider_id) : undefined
  if (!loaded) return null
  if (ready && bound && binding) {
    return (
      <div className="flex items-center gap-2 text-[13px]">
        <Badge variant="ok"><Icon name="check" size={14} />{t('wizard.providersReady')}</Badge>
        <span>{bound.name} · {binding.model}</span>
      </div>
    )
  }
  return (
    <div className="flex max-w-[560px] flex-col gap-3">
      <div className="text-[12.5px] text-orange">{t('wizard.providersBlocked')}</div>
      <ProviderForm doc={doc} onRefresh={onRefresh} />
      <Button size="sm" variant="ghost" className="self-start" onClick={() => api.openSettings().catch(() => {})}>
        <Icon name="settings" size={14} />{t('wizard.goSettings')}
      </Button>
    </div>
  )
}

/** 下拉选中项：已配置供应商 / 内置预设 / 自定义。值编码成 `p:<id>` / `preset:<id>` / `custom`。 */
type Choice = { type: 'provider'; p: ProviderView } | { type: 'preset'; preset: ProviderPreset } | { type: 'custom' }

function usePresets(): ProviderPreset[] {
  const [presets, setPresets] = useState<ProviderPreset[]>([])
  useEffect(() => {
    let live = true
    // 预设只是填表建议：读不到就只剩已配置 + 自定义，不挡这一步。
    api.providerPresets().then((p) => { if (live) setPresets(p) }).catch(() => {})
    return () => { live = false }
  }, [])
  return presets
}

function initialChoice(doc: ProvidersView): string {
  const id = initialProviderId(doc)
  return id ? `p:${id}` : ''
}

function resolveChoice(value: string, doc: ProvidersView, presets: ProviderPreset[]): Choice | null {
  if (value === 'custom') return { type: 'custom' }
  if (value.startsWith('p:')) {
    const p = doc.providers.find((x) => x.id === value.slice(2))
    return p ? { type: 'provider', p } : null
  }
  if (value.startsWith('preset:')) {
    const preset = presets.find((x) => x.id === value.slice(7))
    return preset ? { type: 'preset', preset } : null
  }
  return null
}

/** 服务商下拉（gap B12 2026-10-15，entry#wz-provider「A Anthropic ▾」）：选项带 18 字母头像，
 *  原生 <option> 画不了头像，改走 A9 触发 + A16 弹层的 Select（键盘 / 读屏走 Radix Select）。
 *  头像确定色按供应商 id 哈希，与设置「模型服务」列表同一色号；「自定义…」用加号图标块。 */
function ProviderSelect({ value, onChange, providers, presets }: {
  value: string
  onChange: (v: string) => void
  providers: ProviderView[]
  presets: ProviderPreset[]
}) {
  const { t } = useTranslation()
  const face = (id: string, name: string) => <Avatar agentId={id} name={name} size={18} />
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger aria-label={t('wizard.providerField.label')} data-provider-select="">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {providers.length > 0 && (
          <SelectGroup>
            <SelectLabel>{t('wizard.providerField.configured')}</SelectLabel>
            {providers.map((p) => <SelectItem key={p.id} value={`p:${p.id}`} icon={face(p.id, p.name)}>{p.name}</SelectItem>)}
          </SelectGroup>
        )}
        {presets.length > 0 && (
          <SelectGroup>
            <SelectLabel>{t('wizard.providerField.presets')}</SelectLabel>
            {presets.map((x) => <SelectItem key={x.id} value={`preset:${x.id}`} icon={face(x.id, x.name)}>{x.name}</SelectItem>)}
          </SelectGroup>
        )}
        <SelectSeparator />
        <SelectItem value="custom" icon={
          <span className="grid size-[18px] flex-none place-items-center rounded-[5px] bg-hover-2 text-ink-2"><Icon name="plus" size={12} /></span>
        }>{t('wizard.providerField.custom')}</SelectItem>
      </SelectContent>
    </Select>
  )
}

function ProviderForm({ doc, onRefresh }: { doc: ProvidersView; onRefresh: () => void }) {
  const { t } = useTranslation()
  const presets = usePresets()
  // 已有同 id 供应商的预设不再列——选它就是选那条已配置的，不另建一份。
  // gap B13：预设表与设置目录同一份，向导只取能当 default 主对话槽、要钥匙的子集（wizardPreset）。
  const freshPresets = presets.filter((x) => wizardPreset(x) && !doc.providers.some((p) => p.id === x.id))
  const [picked, setPicked] = useState(() => initialChoice(doc))
  // 空表时默认落第一条预设（原型 entry#wz-provider 默认 Anthropic）；预设异步到达前先空着。
  const value = picked || (freshPresets[0] ? `preset:${freshPresets[0].id}` : 'custom')
  const choice = resolveChoice(value, doc, presets) ?? { type: 'custom' as const }
  const [model, setModel] = useState(() => suggestedModel(doc, doc.providers.find((p) => `p:${p.id}` === picked)))
  const [modelTouched, setModelTouched] = useState(!!picked)
  const [secret, setSecret] = useState('')
  const [name, setName] = useState('')
  const [kind, setKind] = useState<ProviderView['kind']>('openai')
  const [baseUrl, setBaseUrl] = useState('')
  const [busy, setBusy] = useState(false)

  // 预设异步到达后，第一次落到预设的默认模型（负责人还没动过模型框才填）。
  const shownModel = !modelTouched && choice.type === 'preset' ? choice.preset.default_model : model

  const def: ProviderDef | null = choice.type === 'provider'
    ? choice.p
    : choice.type === 'preset'
      ? { id: choice.preset.id, name: choice.preset.name, kind: choice.preset.kind, base_url: choice.preset.base_url, models: [], enabled: true }
      : name.trim() && baseUrl.trim()
        ? { id: providerSlug(name), name: name.trim(), kind, base_url: baseUrl.trim(), models: [], enabled: true }
        : null
  const needsKey = choice.type !== 'provider' || !choice.p.key_set
  const canApply = !!def && !!shownModel.trim() && (!needsKey || !!secret.trim())
  const probe = useKeyProbe(
    def && shownModel.trim() && (!needsKey || secret.trim()) ? def : null,
    needsKey ? secret.trim() : undefined, shownModel)

  function pick(next: string) {
    setPicked(next)
    setSecret('')
    const c = resolveChoice(next, doc, presets)
    setModel(c?.type === 'provider' ? suggestedModel(doc, c.p) : c?.type === 'preset' ? c.preset.default_model : '')
    setModelTouched(true)
  }

  async function apply() {
    if (!canApply || !def) return
    setBusy(true)
    try {
      // 钥匙已存就不要再送 secret——空 secret 后端会留下原钥匙，但这一步根本不该再问。
      await api.saveProvider({ ...def, enabled: true }, needsKey ? secret.trim() : undefined)
      await api.setSlotBinding('default', def.id, shownModel.trim())
      setSecret('')
      onRefresh()
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  const catalog = choice.type === 'provider' ? choice.p.models : []
  return (
    <>
      <Field label={t('wizard.providerField.label')}>
        <ProviderSelect value={value} onChange={pick} providers={doc.providers} presets={freshPresets} />
      </Field>
      {choice.type === 'custom' && (
        <>
          <Field label={t('providers.name')}>
            <Input aria-label={t('providers.name')} placeholder={t('providers.name')} value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field label={t('providers.kind')}>
            <NativeSelect aria-label={t('providers.kind')} value={kind} onChange={(e) => setKind(e.target.value as ProviderView['kind'])}>
              <option value="openai">openai</option>
              <option value="anthropic">anthropic</option>
            </NativeSelect>
          </Field>
          <Field label={t('providers.baseUrl')}>
            <Input aria-label={t('providers.baseUrl')} placeholder={t('providers.baseUrl')} value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
          </Field>
        </>
      )}
      {needsKey && <KeyInput key={value} value={secret} onChange={setSecret} />}
      <ModelProbeField value={shownModel} onChange={(v) => { setModel(v); setModelTouched(true) }} probe={probe}
        list={catalog.length ? 'wizard-default-models' : undefined} />
      {catalog.length > 0 && (
        <datalist id="wizard-default-models">
          {catalog.map((m) => <option key={m.id} value={m.id} />)}
        </datalist>
      )}
      <Button size="sm" variant="ink" className="self-start" disabled={!canApply} loading={busy} onClick={apply}>
        {t('wizard.providersApply')}
      </Button>
    </>
  )
}
