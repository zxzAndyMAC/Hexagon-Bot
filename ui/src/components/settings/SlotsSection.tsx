// 设置「槽位绑定」分区：角色模型槽 → 供应商 + 模型（Cherry「默认模型」位）。
// 2026-10-26 负责人裁决：原先挂在「模型服务」详情栏底部，跟着所选供应商一起出现、要往下翻才看得到；
// 拆成独立分区、导航紧跟「模型服务」之下。被否决：在模型服务页内另起一块——列表 | 详情布局里没有
// 不随所选供应商滚动的位置，槽位表仍会埋在详情下面。
// 数据同源：读写都走 providers.json 的 slots（api.listProviders / setSlotBinding）。
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ProviderPreset, type ProviderView, type ProvidersView, type RoleDef } from '../../api'
import { providerKeyReady } from '../../providerGate'
import { iconKeyOf } from '../../providerIcons'
import { useUiStore } from '../../store'
import { BUILTIN_SLOTS, DEDICATED_PREFIX, isBuiltinSlot, isDedicatedSlot } from '../../modelpick'
import { takeSlotFocus } from '../../settingsSlotFocus'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { NativeSelect } from '@/components/ui/native-select'
import { ProviderIcon } from '../ProviderIcon'
import { SetHead, SetWrap } from './kit'

export function SlotsSection() {
  const { t } = useTranslation()
  const [doc, setDoc] = useState<ProvidersView>({ providers: [], slots: {} })
  const [roles, setRoles] = useState<RoleDef[]>([])
  const [presets, setPresets] = useState<ProviderPreset[]>([])
  const [err, setErr] = useState('')
  // 票 19：配置读回前槽位行会随绑定重挂（SlotRow 的 key 含绑定），聚焦要等首次读回之后。
  const [loaded, setLoaded] = useState(false)

  const reload = useCallback(async () => {
    try {
      setDoc(await api.listProviders())
      setLoaded(true)
    } catch { /* 忽略：空档/读档失败按空表 */ }
  }, [])

  useEffect(() => {
    void reload()
    api.presetRoles().then(setRoles).catch(() => {})
    // 预设只用来给槽位行的服务商取品牌图标；读不到一律中性图标。
    api.providerPresets().then(setPresets).catch(() => {})
  }, [reload])

  // 票 19（规范 E3 补设计）：从时间线「去设置绑定出图模型」进来时，目标槽那一行滚入视区、供应商下拉拿焦点。
  useEffect(() => {
    if (!loaded) return
    const slot = takeSlotFocus()
    if (!slot) return
    const row = [...document.querySelectorAll<HTMLElement>('[data-slot-row]')].find((el) => el.dataset.slotRow === slot)
    row?.scrollIntoView?.({ block: 'center' })
    row?.querySelector('select')?.focus()
  }, [loaded])

  // 槽位集合：预置角色的 model_slot ∪ 产品固定槽 ∪ 已绑定键
  const slotNames = useMemo(() => {
    const s = new Set(roles.map((r) => r.model_slot))
    BUILTIN_SLOTS.forEach((k) => s.add(k))
    Object.keys(doc.slots).forEach((k) => s.add(k))
    return [...s].sort()
  }, [roles, doc.slots])

  const bind = async (slot: string, providerId: string, model: string) => {
    setErr('')
    await api.setSlotBinding(slot, providerId, model).catch((e) => setErr(errText(e)))
    await reload()
  }

  // 槽位绑定汇总：扫一眼知道「还差几步」
  const slotStats = useMemo(() => {
    let bound = 0
    let blocked = 0
    for (const s of slotNames) {
      const b = doc.slots[s]
      if (!b) continue
      bound += 1
      const p = doc.providers.find((x) => x.id === b.provider_id)
      if (!p?.enabled || !providerKeyReady(p)) blocked += 1
    }
    // 票 25：本机回环端点不算缺钥匙
    const missingKey = doc.providers.filter((p) => p.enabled && !providerKeyReady(p)).length
    return { total: slotNames.length, bound, blocked, missingKey }
  }, [slotNames, doc])

  return (
    <SetWrap wide>
      <SetHead
        title={t('settings.nav_slots')}
        desc={t('providers.slotsHint')}
        actions={
          <span className="text-[12px] text-ink-3">
            {t('providers.slotsSummary', { bound: slotStats.bound, total: slotStats.total })}
            {slotStats.missingKey > 0 && <span className="text-orange-ink"> · {t('providers.slotsMissingKey', { count: slotStats.missingKey })}</span>}
          </span>
        }
      />
      {err && <div role="alert" className="mb-3 text-[12.5px] text-red-ink">{err}</div>}
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
          const ready = !!(b && bp?.enabled && providerKeyReady(bp))
          // 2026-09-28 重启后固定槽曾显示空白但标已绑：行先于配置挂载，
          // 仅以 slot 作 key 会保留初始空草稿。已保存绑定变化才重置，普通刷新保留编辑。
          return (
            <SlotRow
              key={JSON.stringify([slot, b?.provider_id, b?.model])}
              slot={slot}
              binding={b}
              bindingProvider={bp}
              providers={doc.providers}
              iconKey={(id) => iconKeyOf(presets, id)}
              ready={ready}
              onBind={bind}
              onUnbind={async (s) => { await api.removeSlotBinding(s).catch(() => {}); await reload() }}
            />
          )
        })}
      </div>
    </SetWrap>
  )
}


/// 一个槽位的绑定行（A21 行形）：槽名 code + 供应商下拉 + 模型输入（datalist 供选）+ 绑定 / 解绑 + 状态。
/// 状态：就绪 ok · 绑定的供应商缺钥匙或停用 warn · default 未绑 warn · 其他未绑走默认回退 neutral。
function SlotRow({
  slot, binding, bindingProvider, providers, iconKey, ready, onBind, onUnbind,
}: {
  slot: string
  binding?: { provider_id: string; model: string }
  bindingProvider?: ProviderView
  providers: ProviderView[]
  iconKey: (providerId: string) => string
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
      {/* #23：原生 <option> 画不了图，槽位表把所选服务商的 16 图标挂在下拉前；未选不画 */}
      <span className="flex min-w-0 items-center gap-1.5">
        {pid && <ProviderIcon iconKey={iconKey(pid)} size={16} />}
        <NativeSelect aria-label={`${label} · ${t('settingsUi.colProvider')}`} className="h-[30px] min-w-0 flex-1" value={pid} onChange={(e) => { setPid(e.target.value); setModel('') }}>
          <option value="">{t('providers.pickProvider')}</option>
          {enabled.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </NativeSelect>
      </span>
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
