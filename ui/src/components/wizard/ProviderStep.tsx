// ① 模型服务商步（票 13，entry#wz-provider）：数据只走 list/save/set_slot_binding，不另起一份供应商存储。
// 放行 = default 槽指向的供应商已启用且钥匙已存、模型名非空；已就绪不渲染钥匙框，缺钥匙才要输入。
// gap B4（2026-10-15）：默认模型框实时检测钥匙（probe_provider_key，输入停 600ms 后请求一次模型目录）：
// 检测中 / 检测通过 · 窗口 / 目录里没有这个模型 / 检测失败 + 原因。检测是建议不是闸——保存仍可点。
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ProviderView, type ProvidersView } from '../../api'
import { useUiStore } from '../../store'
import { providerStepReady } from '../../providerGate'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { Field, Input } from '@/components/ui/input'
import { NativeSelect } from '@/components/ui/native-select'
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
            <Icon name={shown ? 'vision' : 'vision-off'} />
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
    <Field label={t('providers.model')} error={reason} help={unlisted ? t('wizard.probe.unlistedHelp') : undefined}>
      <Input aria-label={t('providers.model')} placeholder={t('providers.pickModel')} value={value} list={list}
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
        <Badge variant="ok"><Icon name="check" />{t('wizard.providersReady')}</Badge>
        <span>{bound.name} · {binding.model}</span>
      </div>
    )
  }
  return (
    <div className="flex max-w-[560px] flex-col gap-3">
      <div className="text-[12.5px] text-orange">{t('wizard.providersBlocked')}</div>
      {doc.providers.length === 0
        ? <NewProviderForm onRefresh={onRefresh} />
        : <FixProviderForm doc={doc} onRefresh={onRefresh} />}
      <Button size="sm" variant="ghost" className="self-start" onClick={() => api.openSettings().catch(() => {})}>
        <Icon name="settings" />{t('wizard.goSettings')}
      </Button>
    </div>
  )
}

function NewProviderForm({ onRefresh }: { onRefresh: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [kind, setKind] = useState<ProviderView['kind']>('openai')
  const [baseUrl, setBaseUrl] = useState('')
  const [secret, setSecret] = useState('')
  const [model, setModel] = useState('')
  const [busy, setBusy] = useState(false)
  const canApply = !!name.trim() && !!baseUrl.trim() && !!secret.trim() && !!model.trim()
  const probe = useKeyProbe(
    baseUrl.trim() && secret.trim() && model.trim()
      ? { id: providerSlug(name || 'new'), name: name.trim(), kind, base_url: baseUrl.trim(), models: [], enabled: true }
      : null,
    secret.trim(), model)

  async function apply() {
    if (!canApply) return
    const id = providerSlug(name)
    setBusy(true)
    try {
      await api.saveProvider({ id, name: name.trim(), kind, base_url: baseUrl.trim(), models: [], enabled: true }, secret.trim())
      await api.setSlotBinding('default', id, model.trim())
      onRefresh()
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  return (
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
      <KeyInput value={secret} onChange={setSecret} />
      <ModelProbeField value={model} onChange={setModel} probe={probe} />
      <Button size="sm" variant="ink" className="self-start" disabled={!canApply} loading={busy} onClick={apply}>
        {t('wizard.providersApply')}
      </Button>
    </>
  )
}

function FixProviderForm({ doc, onRefresh }: { doc: ProvidersView; onRefresh: () => void }) {
  const { t } = useTranslation()
  const [pid, setPid] = useState(() => initialProviderId(doc))
  const provider = doc.providers.find((p) => p.id === pid) ?? doc.providers[0]
  const [model, setModel] = useState(() => suggestedModel(doc, provider))
  const [secret, setSecret] = useState('')
  const [busy, setBusy] = useState(false)
  const needsKey = !provider?.key_set
  const probe = useKeyProbe(
    provider && model.trim() && (!needsKey || secret.trim()) ? provider : null,
    needsKey ? secret.trim() : undefined, model)
  if (!provider) return null
  const canApply = !!model.trim() && (!needsKey || !!secret.trim())

  async function apply() {
    if (!canApply) return
    const modelId = model.trim()
    setBusy(true)
    try {
      // 钥匙已存就不要再送 secret——空 secret 后端会留下原钥匙，但这一步根本不该再问。
      await api.saveProvider({ ...provider, enabled: true }, needsKey ? secret.trim() : undefined)
      await api.setSlotBinding('default', provider.id, modelId)
      setSecret('')
      onRefresh()
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <Field label={t('wizard.providersPick')}>
        <NativeSelect
          aria-label={t('wizard.providersPick')}
          value={provider.id}
          onChange={(e) => {
            const id = e.target.value
            setPid(id)
            setModel(suggestedModel(doc, doc.providers.find((p) => p.id === id)))
            setSecret('')
          }}
        >
          {doc.providers.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </NativeSelect>
      </Field>
      {needsKey && <KeyInput key={provider.id} value={secret} onChange={setSecret} />}
      <ModelProbeField value={model} onChange={setModel} probe={probe} list="wizard-default-models" />
      <datalist id="wizard-default-models">
        {provider.models.map((m) => <option key={m.id} value={m.id} />)}
      </datalist>
      <Button size="sm" variant="ink" className="self-start" disabled={!canApply} loading={busy} onClick={apply}>
        {t('wizard.providersApply')}
      </Button>
    </>
  )
}
