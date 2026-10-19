// 供应商管理：设置页「模型服务」分区（向导服务商步 ProviderStep 另有精简形，只共用 slug 规则）。
// 布局对齐 Cherry Studio：左列=内置常见供应商+已配置供应商+底部固定「添加」；
// 右列=选中供应商详情（密钥/端点/模型目录）——检测、拉取模型列表、能力标记、
// 编辑模型一应俱全；底部「槽位分配」把角色模型槽绑到 供应商+模型。
// 数据语义：非密配置存 providers.json；key 使用 active CredentialStore（provider/<id>）。
import { Fragment, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, asCmdError, errText, type ModelEntry, type ProviderPreset, type ProviderView, type ProvidersView, type RoleDef } from '../api'
import { isLocalEndpoint } from '../providerGate'
import { Icon, type IconName } from './Icon'
import { useUiStore } from '../store'
import { BUILTIN_SLOTS, DEDICATED_PREFIX, isBuiltinSlot, isDedicatedSlot } from '../modelpick'
import { Avatar } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dot } from '@/components/ui/dot'
import { IconButton } from '@/components/ui/icon-button'
import { Field, Input } from '@/components/ui/input'
import { NativeSelect } from '@/components/ui/native-select'
import { Switch } from '@/components/ui/switch'
import { TooltipProvider } from '@/components/ui/tooltip'
import { DetailHead, Empty, ListDetail, SubHead } from './settings/kit'

/// 内置常见供应商目录（未配置时灰显在左列，点选即填右栏默认值）。gap B13（2026-10-15）：原先是这里一份
/// 14 条硬编码表，与向导读的 core provider_presets（5 条）各说各话；现在两边只读 core 那一份。
const CAP_ICONS: Record<string, IconName> = {
  web: 'web', vision: 'vision', reasoning: 'reasoning', tools: 'tool', free: 'free',
  text_generation: 'tool', classification: 'tool', ocr: 'vision', json_mode: 'tool',
  embedding: 'bolt', rerank: 'bolt', image_generation: 'vision', audio_input: 'bolt',
  audio_output: 'bolt', video_input: 'vision', video_generation: 'vision', file_input: 'tool',
  structured_output: 'tool', moderation: 'tool', batch: 'tool', citations: 'tool', code_execution: 'tool',
}
const CAPS = Object.keys(CAP_ICONS)

// context-window 票 02：窗口值显示为 64k / 1M 形；null = 未识别（撞限闸回落 1M）。
const fmtWindow = (v: number) =>
  v >= 1_000_000 ? `${(v / 1_000_000).toFixed(v % 1_000_000 === 0 ? 0 : 1)}M` : `${Math.round(v / 1024)}k`

const slug = (s: string) =>
  s.trim().toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, '-').replace(/^-+|-+$/g, '') || `p${Date.now()}`

function emptyDef(): ProviderView {
  return { id: '', name: '', kind: 'openai', base_url: '', models: [], enabled: true, key_set: false }
}

function emptyModel(): ModelEntry {
  return { id: '', name: '', group: '', caps: [], context_window: 1_000_000, max_output: null }
}

export function ProviderManager() {
  const { t, i18n } = useTranslation()
  const [doc, setDoc] = useState<ProvidersView>({ providers: [], slots: {} })
  const [roles, setRoles] = useState<RoleDef[]>([])
  const [presets, setPresets] = useState<ProviderPreset[]>([])
  const [sel, setSel] = useState<string>('') // provider id / 'preset:<name>' / 'new'
  const [draft, setDraft] = useState<ProviderView | null>(null)
  const [secret, setSecret] = useState('')
  const [showKey, setShowKey] = useState(false)
  const [probe, setProbe] = useState<{ ok: boolean; text: string } | null>(null)
  const [editModel, setEditModel] = useState<ModelEntry | null>(null)
  // Owner 2026-10-01: keep the editor anchored even while its model ID is being edited.
  const [editingId, setEditingId] = useState<string | null>(null)
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)
  const [fetching, setFetching] = useState(false)

  const reload = async (keepSel?: string) => {
    try {
      const d = await api.listProviders()
      setDoc(d)
      const s = keepSel ?? sel
      if (s && !s.startsWith('preset:') && s !== 'new') {
        const p = d.providers.find((x) => x.id === s)
        if (p) setDraft({ ...p })
      }
    } catch { /* 忽略：空档/读档失败按空表 */ }
  }

  useEffect(() => {
    reload()
    api.presetRoles().then(setRoles).catch(() => {})
    // 预设只是填表建议：读不到就只剩已配置 + 「添加供应商」手填，不挡这一页。
    api.providerPresets().then(setPresets).catch(() => {})
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // 已配置的按 id（预设 id 即保存后的供应商 id）或同名（gap B13 前按名称 slug 存下的旧供应商）排除。
  const configuredNames = useMemo(() => new Set(doc.providers.map((p) => p.name)), [doc])
  const unconfiguredPresets = presets.filter((p) =>
    !configuredNames.has(p.name) && !doc.providers.some((x) => x.id === p.id))
  const selConfigured = draft ? doc.providers.some((p) => p.id === draft.id) : false

  const pick = (s: string) => {
    setSel(s)
    setSecret('')
    setProbe(null)
    setEditModel(null)
    setErr('')
    if (s === 'new') {
      setDraft(emptyDef())
    } else if (s.startsWith('preset:')) {
      const p = presets.find((x) => x.id === s.slice(7))
      setDraft(p ? {
        ...emptyDef(),
        id: p.id,
        name: p.name,
        kind: p.kind,
        base_url: p.base_url,
        models: p.default_model ? [{ ...emptyModel(), id: p.default_model, name: null, group: null }] : [],
      } : null)
    } else {
      const p = doc.providers.find((x) => x.id === s)
      setDraft(p ? { ...p } : null)
    }
  }

  const save = async (withSecret?: string) => {
    if (!draft) return
    setBusy(true)
    setErr('')
    try {
      const def = { ...draft, id: draft.id || slug(draft.name) }
      await api.saveProvider(def, withSecret)
      // 执行判定读 jev 槽。第一次保存只绑这一槽。项目经理的 decision 槽是另一件事，
      // 同一次保存再写它，两槽会一起被绑上。
      if (def.kind === 'jev' && def.enabled && !doc.slots.jev) {
        const model = def.models[0]?.id || 'jev-latest'
        await api.setSlotBinding('jev', def.id, model)
      }
      setSecret('')
      await reload(def.id)
    } catch (e) {
      setErr(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const toggleEnabled = async (p: ProviderView) => {
    await api.saveProvider({ ...p, enabled: !p.enabled }).catch((e) => setErr(errText(e)))
    await reload()
  }

  const del = async () => {
    if (!draft) return
    await api.deleteProvider(draft.id).catch((e) => setErr(errText(e)))
    setSel('')
    setDraft(null)
    await reload('')
  }

  /// 拉取模型目录失败的原因文案。owner-followups-1015 #03：core 的码面与连接检测同词，
  /// 文案复用向导的 `wizard.probe.reason.*`；缺钥匙单列——向导那句「输入后自动检测」在设置页不成立。
  const catalogReason = (e: unknown) => {
    const { code } = asCmdError(e)
    if (code === 'missing_credential') return t('providers.missingKeyReason')
    const key = `wizard.probe.reason.${code}`
    return i18n.exists(key) ? t(key) : errText(e)
  }

  /// review R3（settings#models「检测并拉取模型」，原型一个按钮）：曾是「检测」+ 模型目录头「拉取列表」两个按钮，
  /// 实际调同一函数，合为一个。owner-followups-1015 #03：合并后曾分两段——先 probe_provider_key 检测连接、
  /// 再 fetch_provider_models 拉目录——两段请求的是同一个模型目录，一次点击打两次服务商。现在只拉一次：
  /// 失败码即连接检测的原因词（core `AdminError::Catalog`），成功合并进 draft.models（保留已有能力 / 手填窗口）。
  /// 向导只检测不拉取，仍用 probe_provider_key。
  const probeAndMerge = async () => {
    if (!draft) return
    setBusy(true)
    setFetching(true)
    setProbe(null)
    try {
      await save(secret.trim() || undefined) // 先落配置+key 再拉取
      const id = draft.id || slug(draft.name)
      const ids = await api.fetchProviderModels(id)
      const byId = new Map(draft.models.map((m) => [m.id, m]))
      const merged = ids.map((m) => {
        const old = byId.get(m.id)
        // 票 02：窗口/输出上限同理——用户手填值不被拉取覆盖，空值才吃推断。
        return old ? { ...m, caps: old.caps, name: old.name ?? m.name, group: old.group ?? m.group, context_window: old.context_window ?? m.context_window, max_output: old.max_output ?? m.max_output } : m
      })
      const def = { ...draft, id: draft.id || slug(draft.name), models: merged }
      await api.saveProvider(def)
      setDraft(def)
      setProbe({ ok: true, text: t('providers.checkOk', { count: ids.length }) })
      await reload(def.id)
    } catch (e) {
      setProbe({ ok: false, text: catalogReason(e) })
    } finally {
      setBusy(false)
      setFetching(false)
    }
  }

  const saveModel = async (m: ModelEntry) => {
    if (!draft) return
    const models = editingId === null
      ? [...draft.models.filter((x) => x.id !== m.id), m]
      : draft.models.filter((x) => x.id !== m.id || x.id === editingId).map((x) => x.id === editingId ? m : x)
    const def = { ...draft, models }
    setDraft(def)
    setEditModel(null)
    if (selConfigured) {
      await api.saveProvider(def).catch((e) => setErr(errText(e)))
      await reload(def.id)
    }
  }

  const refreshModelMetadata = async () => {
    if (!draft || !editModel) return
    const id = editModel.id
    setBusy(true)
    setErr('')
    try {
      const models = await api.fetchProviderModels(draft.id)
      const model = models.find((m) => m.id === id)
      if (!model) throw new Error(t('providers.modelNotFound'))
      setEditModel((current) => current?.id === id ? {
        ...current, caps: model.caps, context_window: model.context_window, max_output: model.max_output,
      } : current)
    } catch (e) { setErr(catalogReason(e)) }
    finally { setBusy(false) }
  }

  const removeModel = async (id: string) => {
    if (!draft) return
    const def = { ...draft, models: draft.models.filter((x) => x.id !== id) }
    setDraft(def)
    if (selConfigured) {
      await api.saveProvider(def).catch((e) => setErr(errText(e)))
      await reload(def.id)
    }
  }

  // 模型分组：entry.group 优先，缺省按 id 前缀
  const groups = useMemo(() => {
    const g = new Map<string, ModelEntry[]>()
    for (const m of draft?.models ?? []) {
      const k = m.group || m.id.split('/')[0] || 'other'
      g.set(k, [...(g.get(k) ?? []), m])
    }
    return [...g.entries()].sort(([a], [b]) => a.localeCompare(b))
  }, [draft])

  // 槽位集合：预置角色的 model_slot ∪ 产品固定槽 ∪ 已绑定键
  const slotNames = useMemo(() => {
    const s = new Set(roles.map((r) => r.model_slot))
    BUILTIN_SLOTS.forEach((k) => s.add(k))
    Object.keys(doc.slots).forEach((k) => s.add(k))
    return [...s].sort()
  }, [roles, doc.slots])

  const bind = async (slot: string, providerId: string, model: string) => {
    await api.setSlotBinding(slot, providerId, model).catch((e) => setErr(errText(e)))
    await reload()
  }

  const previewUrl = (() => {
    if (!draft) return ''
    const base = draft.base_url.replace(/\/+$/, '')
    if (draft.kind === 'anthropic') return `${base}/v1/messages`
    if (draft.kind === 'jev') return `${base}/v1/systemone`
    return `${base}/chat/completions`
  })()

  // ui-audit-2 票 10：本地端点（Ollama/LM Studio）不需要 key——检测不禁用
  const needsKey = !!draft && !isLocalEndpoint(draft.base_url)
  const probeDisabled = busy || !draft || (needsKey && !draft.key_set && !secret.trim())

  // 槽位绑定汇总：扫一眼知道「还差几步」
  const slotStats = useMemo(() => {
    let bound = 0
    let blocked = 0
    for (const s of slotNames) {
      const b = doc.slots[s]
      if (!b) continue
      bound += 1
      const p = doc.providers.find((x) => x.id === b.provider_id)
      if (!p?.enabled || !p.key_set) blocked += 1
    }
    const missingKey = doc.providers.filter((p) => p.enabled && !p.key_set).length
    return { total: slotNames.length, bound, blocked, missingKey }
  }, [slotNames, doc])

  // ui-redesign 票 11（settings#models，components E2 + E3 + A21）：列表 =「已配置 / 内置目录」两组（状态点
  // ● 已存钥匙 / ◐ 缺钥匙橙 / ○ 停用 + 字母头像 + 行内开关），详情 = 头（大字母头像 + 名 + 端点「· 自动推导」+ 删除）、
  // API Key 与「检测并拉取模型」、模型卡（等宽名 + 窗口 chip + 能力图标 + 编辑）、槽位绑定表。
  type Item = { key: string; preset: boolean; p: ProviderView | ProviderPreset }
  const items: Item[] = [
    ...doc.providers.map((p) => ({ key: p.id, preset: false, p })),
    ...unconfiguredPresets.map((p) => ({ key: `preset:${p.id}`, preset: true, p })),
  ]

  const modelEditor = editModel && (
    <div data-model-editor className="mt-1 mb-2 rounded-[var(--r-card)] bg-surface p-3.5 shadow-[var(--focus-ring)]">
      <div className="grid grid-cols-[96px_1fr] items-center gap-x-2.5 gap-y-2 text-[12.5px]">
        <label className="text-ink-3">{t('providers.modelId')}</label>
        <Input className="h-[30px] font-mono" aria-label={t('providers.modelId')} value={editModel.id} onChange={(e) => setEditModel({ ...editModel, id: e.target.value })} />
        <label className="text-ink-3">{t('providers.modelName')}</label>
        <Input className="h-[30px]" aria-label={t('providers.modelName')} value={editModel.name ?? ''} onChange={(e) => setEditModel({ ...editModel, name: e.target.value })} />
        <label className="text-ink-3">{t('providers.modelGroup')}</label>
        <Input className="h-[30px]" aria-label={t('providers.modelGroup')} value={editModel.group ?? ''} onChange={(e) => setEditModel({ ...editModel, group: e.target.value })} />
        <label className="self-start pt-1 text-ink-3">{t('providers.caps')}</label>
        <div className="flex flex-wrap gap-x-3 gap-y-1.5">
          {CAPS.map((c) => (
            <label key={c} className="flex cursor-pointer items-center gap-1 text-[12px] text-ink-2">
              <input
                type="checkbox"
                className="accent-[var(--accent)]"
                checked={editModel.caps.includes(c)}
                onChange={(e) =>
                  setEditModel({
                    ...editModel,
                    caps: e.target.checked ? [...editModel.caps, c] : editModel.caps.filter((x) => x !== c),
                  })
                }
              />
              <Icon name={CAP_ICONS[c]} size={12} /> {t(`providers.cap_${c}`)}
            </label>
          ))}
        </div>
        {/* 票 02：撞限闸/输出上限的模型元数据（ADR 0068），留空走回落 */}
        <label className="text-ink-3">{t('providers.modelWindow')}</label>
        <Input
          className="h-[30px] font-mono" type="number" min={0} step={1024}
          aria-label={t('providers.modelWindow')} placeholder="1000000"
          value={editModel.context_window ?? ''}
          onChange={(e) => setEditModel({ ...editModel, context_window: e.target.value === '' ? null : Number(e.target.value) })}
        />
        <label className="text-ink-3">{t('providers.modelMaxOut')}</label>
        <Input
          className="h-[30px] font-mono" type="number" min={0} step={1024}
          aria-label={t('providers.modelMaxOut')}
          value={editModel.max_output ?? ''}
          onChange={(e) => setEditModel({ ...editModel, max_output: e.target.value === '' ? null : Number(e.target.value) })}
        />
      </div>
      <p className="mt-2 text-[12px] text-ink-3">{t('providers.capsHint')}</p>
      <p className="mt-1 text-[12px] text-ink-3">{t('providers.metaHint')}</p>
      <div className="mt-3 flex gap-2">
        {selConfigured && <Button size="xs" variant="ghost" disabled={busy || !editModel.id.trim()} onClick={refreshModelMetadata}>{t('providers.refreshMetadata')}</Button>}
        <span className="flex-1" />
        <Button size="xs" variant="quiet" disabled={busy} onClick={() => setEditModel(null)}>{t('providers.cancel')}</Button>
        <Button size="xs" variant="primary" disabled={busy || !editModel.id.trim()} onClick={() => saveModel(editModel)}>{t('providers.save')}</Button>
      </div>
    </div>
  )

  const slots = (
    <>
      <SubHead aside={
        <span className="text-[12px] text-ink-3">
          {t('providers.slotsSummary', { bound: slotStats.bound, total: slotStats.total })}
          {slotStats.missingKey > 0 && <span className="text-orange-ink"> · {t('providers.slotsMissingKey', { count: slotStats.missingKey })}</span>}
        </span>
      }>{t('providers.slots')}</SubHead>
      <p className="-mt-1 mb-2 text-[12px] text-ink-3">{t('providers.slotsHint')}</p>
      <div aria-label={t('providers.slots')} className="rounded-[var(--r-card)] bg-[var(--app-card)] text-[13px] shadow-[var(--shadow-card)]">
        <div className="grid grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)_minmax(0,1.3fr)_112px_92px] items-center gap-2 border-b border-line px-3 py-[9px] text-[12px] font-medium text-ink-3">
          <span>{t('providers.slot')}</span>
          <span>{t('settingsUi.colProvider')}</span>
          <span>{t('providers.model')}</span>
          <span><span className="sr-only">{t('providers.bind')}</span></span>
          <span>{t('settingsUi.colStatus')}</span>
        </div>
        {slotNames.map((slot) => {
          const b = doc.slots[slot]
          const bp = b ? doc.providers.find((p) => p.id === b.provider_id) : undefined
          const ready = !!(b && bp?.enabled && bp.key_set)
          // 2026-09-28 重启后固定槽曾显示空白但标已绑：行先于配置挂载，
          // 仅以 slot 作 key 会保留初始空草稿。已保存绑定变化才重置，普通刷新保留编辑。
          return (
            <SlotRow
              key={JSON.stringify([slot, b?.provider_id, b?.model])}
              slot={slot}
              binding={b}
              bindingProvider={bp}
              providers={doc.providers}
              ready={ready}
              onBind={bind}
              onUnbind={async (s) => { await api.removeSlotBinding(s).catch(() => {}); await reload() }}
            />
          )
        })}
      </div>
    </>
  )

  // TooltipProvider：本组件也被单独挂载（ProviderManager.test），IconButton 的 tooltip 需要 Provider
  return (
    <TooltipProvider>
    <ListDetail<Item>
      label={t('settings.nav_models')}
      items={items}
      itemKey={(it) => it.key}
      filterText={(it) => it.p.name}
      groupOf={(it) => (it.preset ? 'preset' : 'configured')}
      groupLabel={(g) => t(g === 'preset' ? 'settingsUi.providersCatalog' : 'settingsUi.providersConfigured')}
      itemDim={(it) => !it.preset && !(it.p as ProviderView).enabled}
      selected={sel && sel !== 'new' ? sel : null}
      onSelect={(k) => { if (k) pick(k) }}
      searchPlaceholder={t('providers.filter')}
      emptyText={t('providers.empty')}
      foot={
        <Button size="sm" className="flex-1" onClick={() => pick('new')}><Icon name="plus" size={14} />{t('providers.add')}</Button>
      }
      renderItem={(it) => {
        if (it.preset) {
          return (
            <>
              <span className="grid size-[18px] flex-none place-items-center rounded-[5px] bg-hover-2 text-[10px] font-semibold text-ink-2">{it.p.name.slice(0, 1)}</span>
              <div className="min-w-0 flex-1 truncate text-ink-3">{it.p.name}</div>
              {isLocalEndpoint(it.p.base_url) && <span className="text-[12px] text-ink-3">{t('settingsUi.local')}</span>}
            </>
          )
        }
        const p = it.p as ProviderView
        return (
          <>
            {/* 配置状态点：●已配 key / ◐缺 key（橙）/ ○停用——不用点进去就知道能不能用 */}
            <span className="inline-flex" title={!p.enabled ? t('providers.disabledTag') : p.key_set ? t('providers.keySet') : t('providers.keyMissing')}>
              <Dot tone={!p.enabled ? 'off' : p.key_set || isLocalEndpoint(p.base_url) ? 'on' : 'warn'} />
            </span>
            <Avatar agentId={p.id} name={p.name} size={18} />
            <div className="min-w-0 flex-1 truncate">{p.name}</div>
            <Switch
              aria-label={`${t('providers.enabled')} ${p.name}`}
              checked={p.enabled}
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => e.stopPropagation()}
              onCheckedChange={() => toggleEnabled(p)}
            />
          </>
        )
      }}
    >
      <div data-provider-detail className="max-w-[720px]">
        {!draft ? (
          <Empty icon="cpu" title={t('settings.nav_models')} desc={t('providers.hint')} />
        ) : (
          <>
            <DetailHead
              lead={<Avatar agentId={draft.id || draft.name} name={draft.name || '?'} size={40} />}
              title={draft.name || t('providers.add')}
              sub={draft.base_url ? `${previewUrl} · ${t('providers.autoUrl')}` : t('providers.unsaved')}
              mono={!!draft.base_url}
              actions={selConfigured && (
                <Button size="sm" variant="ghost" className="text-red-ink" onClick={del}><Icon name="trash" size={14} />{t('providers.delete')}</Button>
              )}
            />
            <div className="mt-4 grid grid-cols-2 gap-3">
              <Field label={t('providers.name')}>
                <Input aria-label={t('providers.name')} value={draft.name} placeholder={t('providers.name')}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
              </Field>
              <Field label={t('providers.kind')} help={draft.kind === 'jev' ? t('providers.jevHint') : undefined}>
                <NativeSelect aria-label={t('providers.kind')} value={draft.kind}
                  onChange={(e) => setDraft({ ...draft, kind: e.target.value as ProviderView['kind'] })}>
                  <option value="openai">{t('providers.kindOpenai')}</option>
                  <option value="anthropic">Anthropic</option>
                  <option value="jev">Jev</option>
                </NativeSelect>
              </Field>
            </div>
            <Field className="mt-3" label={t('providers.baseUrl')}>
              <Input aria-label={t('providers.baseUrl')} className="font-mono text-[12px]" value={draft.base_url} placeholder="https://…/v1"
                onChange={(e) => setDraft({ ...draft, base_url: e.target.value })} />
            </Field>
            <div className="mt-3 grid grid-cols-[1fr_auto] items-end gap-3">
              <Field label={t('providers.key')} help={draft.key_set ? t('providers.keyKeep') : t('providers.keyMissing')}>
                <Input
                  aria-label={t('providers.key')}
                  className="font-mono"
                  type={showKey ? 'text' : 'password'}
                  value={secret}
                  placeholder={draft.key_set ? '••••••••' : 'sk-…'}
                  onChange={(e) => setSecret(e.target.value)}
                  trailing={
                    <IconButton size="sm" label={t(showKey ? 'providers.hideKey' : 'providers.showKey')} onClick={() => setShowKey((s) => !s)}>
                      <Icon name={showKey ? 'vision' : 'vision-off'} size={14} />
                    </IconButton>
                  }
                />
              </Field>
              <Button
                className="mb-[22px]" disabled={probeDisabled} loading={busy} onClick={probeAndMerge}
                title={probeDisabled && !busy ? t('providers.checkNeedsKey') : undefined}
              >
                {!busy && <Icon name="refresh" size={14} />}{t(fetching ? 'providers.fetch' : 'providers.check')}
              </Button>
            </div>
            {probe && (
              <div role="status" className={`mt-1 text-[12.5px] ${probe.ok ? 'text-green-ink' : 'text-red-ink'}`}>{probe.text}</div>
            )}

            {/* ---- 模型目录 ---- */}
            <SubHead aside={
              <>
                <Button size="xs" variant="ghost" onClick={() => { setEditingId(null); setEditModel(emptyModel()) }}><Icon name="plus" size={14} />{t('providers.addModel')}</Button>
              </>
            }>{t('settingsUi.modelsN', { count: draft.models.length })}</SubHead>
            {editingId === null && modelEditor}
            {draft.models.length === 0 ? (
              <div className="py-2 text-[12.5px] text-ink-3">{t('providers.noModels')}</div>
            ) : (
              <div className="rounded-[var(--r-card)] bg-[var(--app-card)] shadow-[var(--shadow-card)]">
                {groups.map(([g, ms]) => (
                  <div key={g} className="border-t border-line-soft first:border-t-0">
                    {groups.length > 1 && (
                      <button type="button" aria-expanded={!collapsed[g]}
                        className="hx-focus flex w-full cursor-pointer items-center gap-1 px-3.5 pt-2 pb-1 text-left text-[12px] text-ink-3"
                        onClick={() => setCollapsed((c) => ({ ...c, [g]: !c[g] }))}>
                        <Icon name={collapsed[g] ? 'chevron-right' : 'chevron-down'} size={12} /> {g}
                      </button>
                    )}
                    {!collapsed[g] && ms.map((m) => (
                      <Fragment key={m.id}>
                        <div data-model-row={m.id} className="flex min-h-11 items-center gap-2 border-t border-line-soft px-3.5 py-2 first:border-t-0">
                          <b className="min-w-0 flex-1 truncate font-mono text-[12.5px] font-medium text-ink">{m.name || m.id}</b>
                          {/* 票 02：窗口/输出元数据可见性——未识别挂警告标，撞限闸回落 1M 要让人知道 */}
                          {m.context_window != null ? (
                            <Badge title={t('providers.modelWindow')} className="tabular-nums">{fmtWindow(m.context_window)}</Badge>
                          ) : (
                            <Badge variant="warn" title={t('providers.windowUnknownHint')}><Icon name="warn" size={12} />{t('providers.windowUnknown')}</Badge>
                          )}
                          {m.max_output != null && (
                            <span className="text-[12px] text-ink-3 tabular-nums" title={t('providers.modelMaxOut')}>↓{fmtWindow(m.max_output)}</span>
                          )}
                          {m.caps.map((c) => (
                            <span key={c} title={t(`providers.cap_${c}`)} className="inline-flex text-ink-3">
                              <Icon name={CAP_ICONS[c] ?? 'bolt'} size={14} />
                            </span>
                          ))}
                          <IconButton size="sm" label={`${t('providers.edit')} ${m.id}`} onClick={() => { setEditingId(m.id); setEditModel({ ...m }) }}>
                            <Icon name="edit" size={12} />
                          </IconButton>
                          <IconButton size="sm" label={`${t('providers.delete')} ${m.id}`} className="text-red" onClick={() => removeModel(m.id)}>
                            <Icon name="close" size={12} />
                          </IconButton>
                        </div>
                        {editingId === m.id && modelEditor}
                      </Fragment>
                    ))}
                  </div>
                ))}
              </div>
            )}

            {err && <div role="alert" className="mt-2 text-[12.5px] text-red-ink">{err}</div>}
            <div className="mt-4 flex items-center gap-2">
              {!selConfigured && <span className="text-[12px] text-ink-3">{t('providers.unsaved')}</span>}
              <span className="flex-1" />
              <Button size="sm" variant="primary" disabled={busy || !draft.name.trim() || !draft.base_url.trim()}
                onClick={() => save(secret.trim() || undefined)}>
                {t('providers.save')}
              </Button>
            </div>
          </>
        )}
        {/* ---- 槽位分配：角色模型槽 → 供应商+模型（Cherry「默认模型」位） ---- */}
        {slots}
      </div>
    </ListDetail>
    </TooltipProvider>
  )
}

/// 一个槽位的绑定行（A21 行形）：槽名 code + 供应商下拉 + 模型输入（datalist 供选）+ 绑定 / 解绑 + 状态。
/// 状态：就绪 ok · 绑定的供应商缺钥匙或停用 warn · default 未绑 warn · 其他未绑走默认回退 neutral。
function SlotRow({
  slot, binding, bindingProvider, providers, ready, onBind, onUnbind,
}: {
  slot: string
  binding?: { provider_id: string; model: string }
  bindingProvider?: ProviderView
  providers: ProviderView[]
  ready: boolean
  onBind: (slot: string, providerId: string, model: string) => void
  onUnbind: (slot: string) => void
}) {
  const { t } = useTranslation()
  const [pid, setPid] = useState(binding?.provider_id ?? '')
  const [model, setModel] = useState(binding?.model ?? '')
  const provider = providers.find((p) => p.id === pid)
  const enabled = providers.filter((p) => p.enabled)
  // ui-audit-2 票 01：agent:<id> 专属槽是角色编辑器写出的内部名，
  // 槽位表里翻译成「专属 · 角色名」而非裸 id。
  const team = useUiStore((s) => s.team)
  // 产品固定槽给本地化名（BUILTIN_SLOTS 词表，负责人反馈 2026-09：槽名全英文），
  // 原槽名小字保留作配置键参照；角色自定义槽名是模板数据，原文照显。
  const label = isDedicatedSlot(slot)
    ? `${t('agent.dedicatedTag')} · ${team.find((m) => m.id === slot.slice(DEDICATED_PREFIX.length))?.role ?? slot}`
    : isBuiltinSlot(slot)
      ? t(`providers.slot_${slot}`)
      : slot
  // jev 与 image（出图，功能缺口 C2）只认精确绑定：未绑就是空，不写「回落默认」。
  const exact = slot === 'jev' || slot === 'image'
  const state = binding ? 'bound' : exact ? 'empty' : 'default'
  const chip = binding
    ? ready
      ? <Badge variant="ok">{t('settingsUi.slotReady')}</Badge>
      : <Badge variant="warn">{bindingProvider && !bindingProvider.enabled ? t('providers.disabledTag') : t('settingsUi.slotMissingKey')}</Badge>
    : exact
      ? <Badge>{t(slot === 'jev' ? 'providers.jevEmpty' : 'providers.imageEmpty')}</Badge>
      : slot === 'default'
        ? <Badge variant="warn">{t('providers.unbound')}</Badge>
        : <Badge>{t('settingsUi.slotFallback')}</Badge>
  return (
    <div data-slot-row={slot} className="grid grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)_minmax(0,1.3fr)_112px_92px] items-center gap-2 border-t border-line-soft px-3 py-2 first:border-t-0 live:hover:bg-hover">
      {/* 票 16c（fr 冒烟）：槽位名不截断——「Traduction des prompts」曾被省略号吃掉；名占一行可换行，槽位 id 另起一行。 */}
      <code className="flex min-w-0 flex-col font-sans text-[13px] text-ink" title={isBuiltinSlot(slot) ? `${label} · ${slot}` : label}>
        <span className="[overflow-wrap:anywhere]">{label}</span>
        {isBuiltinSlot(slot) && <span className="truncate font-mono text-[11px] text-ink-3">{slot}</span>}
      </code>
      <NativeSelect aria-label={`${label} · ${t('settingsUi.colProvider')}`} className="h-[30px]" value={pid} onChange={(e) => { setPid(e.target.value); setModel('') }}>
        <option value="">{t('providers.pickProvider')}</option>
        {enabled.map((p) => (
          <option key={p.id} value={p.id}>{p.name}</option>
        ))}
      </NativeSelect>
      <Input
        aria-label={`${label} · ${t('providers.model')}`}
        className="h-[30px] font-mono text-[12px]" list={`models-${slot}`}
        value={model} placeholder={t('providers.pickModel')}
        onChange={(e) => setModel(e.target.value)}
      />
      <datalist id={`models-${slot}`}>
        {provider?.models.map((m) => <option key={m.id} value={m.id} />)}
      </datalist>
      <span className="flex gap-1">
        <Button size="xs" disabled={!pid || !model.trim()} onClick={() => onBind(slot, pid, model.trim())}>{t('providers.bind')}</Button>
        {binding && <Button size="xs" variant="ghost" onClick={() => onUnbind(slot)}>{t('providers.unbind')}</Button>}
      </span>
      {/* data-slot-state 挂在行的直接子元素上：测试按 parentElement 回到行 */}
      <span data-slot-state={state} className="inline-flex justify-end">{chip}</span>
    </div>
  )
}
