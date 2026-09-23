// 供应商管理（共享件）：设置页「模型与凭据」分区与启动页设置同用。
// 布局对齐 Cherry Studio：左列=内置常见供应商+已配置供应商+底部固定「添加」；
// 右列=选中供应商详情（密钥/端点/模型目录）——检测、拉取模型列表、能力标记、
// 编辑模型一应俱全；底部「槽位分配」把角色模型槽绑到 供应商+模型。
// 数据语义：非密配置存 providers.json；key 只写 keychain（provider/<id>）。
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ModelEntry, type ProviderView, type ProvidersView, type RoleDef } from '../api'
import { Icon, type IconName } from './Icon'
import { useUiStore } from '../store'
import { DEDICATED_PREFIX, isDedicatedSlot } from '../modelpick'

/// 内置常见供应商目录（未配置时灰显在左列，点选即填右栏默认值）。
const PRESETS: { name: string; kind: ProviderView['kind']; base_url: string; model?: string }[] = [
  { name: 'TypeSafe', kind: 'jev', base_url: 'https://api.typesafe.ai', model: 'jev-latest' },
  { name: 'OpenAI', kind: 'openai', base_url: 'https://api.openai.com/v1' },
  { name: 'Anthropic', kind: 'anthropic', base_url: 'https://api.anthropic.com' },
  { name: 'OpenRouter', kind: 'openai', base_url: 'https://openrouter.ai/api/v1' },
  { name: 'DeepSeek', kind: 'openai', base_url: 'https://api.deepseek.com/v1' },
  { name: 'SiliconFlow', kind: 'openai', base_url: 'https://api.siliconflow.cn/v1' },
  { name: 'Zhipu', kind: 'openai', base_url: 'https://open.bigmodel.cn/api/paas/v4' },
  { name: 'Moonshot', kind: 'openai', base_url: 'https://api.moonshot.cn/v1' },
  { name: 'xAI', kind: 'openai', base_url: 'https://api.x.ai/v1' },
  { name: 'Groq', kind: 'openai', base_url: 'https://api.groq.com/openai/v1' },
  { name: 'Mistral', kind: 'openai', base_url: 'https://api.mistral.ai/v1' },
  { name: 'Gemini', kind: 'openai', base_url: 'https://generativelanguage.googleapis.com/v1beta/openai' },
  { name: 'Ollama', kind: 'openai', base_url: 'http://localhost:11434/v1' },
  { name: 'LM Studio', kind: 'openai', base_url: 'http://localhost:1234/v1' },
]

const CAP_ICONS: Record<string, IconName> = {
  web: 'web', vision: 'vision', reasoning: 'reasoning', tools: 'tool', free: 'free',
}
const CAPS = ['web', 'vision', 'reasoning', 'tools', 'free'] as const

// context-window 票 02：窗口值显示为 64k / 1M 形；null = 未识别（撞限闸回落 120k）。
const fmtWindow = (v: number) =>
  v >= 1_000_000 ? `${(v / 1_000_000).toFixed(v % 1_000_000 === 0 ? 0 : 1)}M` : `${Math.round(v / 1024)}k`

const slug = (s: string) =>
  s.trim().toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, '-').replace(/^-+|-+$/g, '') || `p${Date.now()}`

function emptyDef(): ProviderView {
  return { id: '', name: '', kind: 'openai', base_url: '', models: [], enabled: true, key_set: false }
}

function emptyModel(): ModelEntry {
  return { id: '', name: '', group: '', caps: [], context_window: null, max_output: null }
}

export function ProviderManager() {
  const { t } = useTranslation()
  const [doc, setDoc] = useState<ProvidersView>({ providers: [], slots: {} })
  const [roles, setRoles] = useState<RoleDef[]>([])
  const [sel, setSel] = useState<string>('') // provider id / 'preset:<name>' / 'new'
  const [draft, setDraft] = useState<ProviderView | null>(null)
  const [secret, setSecret] = useState('')
  const [showKey, setShowKey] = useState(false)
  const [probe, setProbe] = useState<{ ok: boolean; text: string } | null>(null)
  const [editModel, setEditModel] = useState<ModelEntry | null>(null)
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
  const [q, setQ] = useState('') // settings-3col 票 04：供应商搜索（Cherry 同款）
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const configuredNames = useMemo(() => new Set(doc.providers.map((p) => p.name)), [doc])
  const unconfiguredPresets = PRESETS.filter((p) => !configuredNames.has(p.name))
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
      const p = PRESETS.find((x) => x.name === s.slice(7))
      setDraft(p ? {
        ...emptyDef(),
        name: p.name,
        kind: p.kind,
        base_url: p.base_url,
        models: p.model ? [{ ...emptyModel(), id: p.model, name: 'Jev', group: 'typesafe' }] : [],
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
      // 决策模型只给项目经理。第一次保存 Jev 时把 decision 槽绑上，核在刷新供应商时写进决策槽。
      if (def.kind === 'jev' && def.enabled && !doc.slots.jev) {
        const model = def.models[0]?.id || 'jev-latest'
        await api.setSlotBinding('jev', def.id, model)
      }
      if (def.kind === 'jev' && def.enabled && !doc.slots.decision) {
        const model = def.models[0]?.id || 'jev-latest'
        await api.setSlotBinding('decision', def.id, model)
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

  /// 检测 = 拉一次模型目录；成功顺手合并进 draft.models（保留已有能力标记）。
  const probeAndMerge = async () => {
    if (!draft) return
    setBusy(true)
    setProbe(null)
    try {
      await save(secret.trim() || undefined) // 先落配置+key 再拉
      const ids = await api.fetchProviderModels(draft.id || slug(draft.name))
      const byId = new Map(draft.models.map((m) => [m.id, m]))
      const merged = ids.map((m) => {
        const old = byId.get(m.id)
        // 票 02：窗口/输出上限同理——用户手填值不被拉取覆盖，空值才吃推断。
        return old ? { ...m, caps: old.caps.length ? old.caps : m.caps, name: old.name, context_window: old.context_window ?? m.context_window, max_output: old.max_output ?? m.max_output } : m
      })
      const def = { ...draft, id: draft.id || slug(draft.name), models: merged }
      await api.saveProvider(def)
      setDraft(def)
      setProbe({ ok: true, text: t('providers.checkOk', { count: ids.length }) })
      await reload(def.id)
    } catch (e) {
      setProbe({ ok: false, text: errText(e) })
    } finally {
      setBusy(false)
    }
  }

  const saveModel = async (m: ModelEntry) => {
    if (!draft) return
    const models = [...draft.models.filter((x) => x.id !== m.id), m]
    const def = { ...draft, models }
    setDraft(def)
    setEditModel(null)
    if (selConfigured) {
      await api.saveProvider(def).catch((e) => setErr(errText(e)))
      await reload(def.id)
    }
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

  // 槽位集合：预置角色的 model_slot ∪ default ∪ 已绑定键
  const slotNames = useMemo(() => {
    const s = new Set(roles.map((r) => r.model_slot))
    s.add('default')
    s.add('decision')
    s.add('role_draft')
    s.add('brief')
    s.add('flow_draft')
    s.add('jev')
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
  const needsKey = !!draft && !/^(https?:\/\/)?(localhost|127\.0\.0\.1|\[::1\])/i.test(draft.base_url)
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

  const row: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 8, padding: '8px 10px', borderRadius: 8, cursor: 'pointer' }

  return (
    <div>
      <div className="dim3" style={{ fontSize: 12, marginBottom: 10 }}>{t('providers.hint')}</div>
      <div style={{ display: 'flex', gap: 14, alignItems: 'flex-start' }}>
        {/* ---- 左列：供应商列表（Cherry 式，settings-3col 票 04 对齐 ListDetail） ---- */}
        <div
          className="panel"
          style={{ width: 270, flexShrink: 0, display: 'flex', flexDirection: 'column', maxHeight: 440 }}
        >
          <div style={{ padding: '6px 6px 2px' }}>
            <input
              className="input"
              style={{ width: '100%' }}
              placeholder={t('providers.filter')}
              value={q}
              onChange={(e) => setQ(e.target.value)}
            />
          </div>
          <div style={{ flex: 1, overflowY: 'auto', padding: '6px 4px' }}>
            {doc.providers.filter((p) => !q || p.name.toLowerCase().includes(q.toLowerCase())).map((p) => (
              <div
                key={p.id}
                style={{
                  ...row,
                  // 票 10：选中锚定用左竖条+底色（outline 会让行高跳）
                  boxShadow: sel === p.id ? 'inset 2px 0 0 var(--accent)' : 'inset 2px 0 0 transparent',
                  background: sel === p.id ? 'var(--accent-soft)' : undefined,
                  opacity: p.enabled ? 1 : 0.55,
                }}
                onClick={() => pick(p.id)}
              >
                {/* 配置状态点：●已配 key / ◐缺 key（琥珀）/ ○停用——不用点进去就知道能不能用 */}
                <span
                  className={`dot ${!p.enabled ? 'off' : p.key_set ? 'on' : 'warn'}`}
                  title={!p.enabled ? t('providers.disabledTag') : p.key_set ? t('providers.keySet') : t('providers.keyMissing')}
                />
                <div style={{ flex: 1, minWidth: 0, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {p.name}
                </div>
                {!p.enabled && <span className="chip" style={{ fontSize: 10 }}>{t('providers.disabledTag')}</span>}
                <input
                  type="checkbox"
                  className="switch"
                  checked={p.enabled}
                  title={t('providers.enabled')}
                  onClick={(e) => e.stopPropagation()}
                  onChange={() => toggleEnabled(p)}
                />
              </div>
            ))}
            {unconfiguredPresets.filter((p) => !q || p.name.toLowerCase().includes(q.toLowerCase())).length > 0
              && doc.providers.length > 0 && (
              <div style={{ borderTop: '1px solid var(--border)', margin: '4px 6px' }} />
            )}
            {unconfiguredPresets.filter((p) => !q || p.name.toLowerCase().includes(q.toLowerCase())).map((p) => (
              <div
                key={p.name}
                className="dim3"
                style={{
                  ...row,
                  boxShadow: sel === `preset:${p.name}` ? 'inset 2px 0 0 var(--accent)' : 'inset 2px 0 0 transparent',
                  background: sel === `preset:${p.name}` ? 'var(--accent-soft)' : undefined,
                }}
                onClick={() => pick(`preset:${p.name}`)}
              >
                <div style={{ flex: 1, fontSize: 13 }}>{p.name}</div>
                <span className="chip" style={{ fontSize: 10 }}>{t('providers.preset')}</span>
              </div>
            ))}
          </div>
          <div style={{ borderTop: '1px solid var(--border)', padding: 6 }}>
            <button
              className="btn" style={{ width: '100%', fontSize: 12, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 4 }}
              onClick={() => pick('new')}
            >
              <Icon name="plus" size={11} /> {t('providers.add')}
            </button>
          </div>
        </div>

        {/* ---- 右列：供应商详情（ui-polish-3：统一 .panel 圆角边线卡） ---- */}
        <div style={{ flex: 1, minWidth: 0 }}>
          {!draft && <div className="dim3" style={{ fontSize: 12, padding: '30px 0', textAlign: 'center' }}>{t('providers.pickHint')}</div>}
          {draft && (
            <div className="panel" style={{ padding: '12px 14px' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
                <input
                  className="input" style={{ fontWeight: 560, flex: '0 1 240px' }}
                  value={draft.name} placeholder={t('providers.name')}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                />
                <span className="chip">{draft.kind}</span>
                <span className="chip" style={draft.key_set ? { color: 'var(--ok)' } : { color: 'var(--err)' }}>
                  {draft.key_set ? t('providers.keySet') : t('providers.keyMissing')}
                </span>
                <div style={{ flex: 1 }} />
                {selConfigured && (
                  <button className="btn" style={{ fontSize: 12, color: 'var(--err)' }} onClick={del}>{t('providers.delete')}</button>
                )}
              </div>

              <label className="dim3" style={{ fontSize: 12 }}>{t('providers.key')}</label>
              <div style={{ display: 'flex', gap: 6, margin: '4px 0 10px' }}>
                <input
                  className="input mono" style={{ flex: 1 }}
                  type={showKey ? 'text' : 'password'}
                  value={secret}
                  placeholder={draft.key_set ? t('providers.keyKeep') : 'sk-…'}
                  onChange={(e) => setSecret(e.target.value)}
                />
                <button className="btn" title={t(showKey ? 'providers.hideKey' : 'providers.showKey')} onClick={() => setShowKey((s) => !s)}>
                  <Icon name={showKey ? 'vision' : 'vision-off'} size={12} />
                </button>
                <button
                  className="btn" disabled={probeDisabled} onClick={probeAndMerge}
                  title={probeDisabled && !busy ? t('providers.checkNeedsKey') : undefined}
                >
                  {t('providers.check')}
                </button>
              </div>
              {probe && (
                <div style={{ fontSize: 12, marginBottom: 8, color: probe.ok ? 'var(--ok)' : 'var(--err)' }}>{probe.text}</div>
              )}

              <label className="dim3" style={{ fontSize: 12 }}>{t('providers.kind')}</label>
              <select
                className="input" style={{ margin: '4px 0 10px', width: 'auto', minWidth: 220 }}
                value={draft.kind}
                onChange={(e) => setDraft({ ...draft, kind: e.target.value as ProviderView['kind'] })}
              >
                <option value="openai">OpenAI 兼容</option>
                <option value="anthropic">Anthropic</option>
                <option value="jev">Jev</option>
              </select>
              {draft.kind === 'jev' && (
                <div className="dim3" style={{ fontSize: 12, marginTop: -6, marginBottom: 10 }}>{t('providers.jevHint')}</div>
              )}

              <label className="dim3" style={{ fontSize: 12 }}>{t('providers.baseUrl')}</label>
              <input
                className="input mono" style={{ marginTop: 4 }}
                value={draft.base_url} placeholder="https://…/v1"
                onChange={(e) => setDraft({ ...draft, base_url: e.target.value })}
              />
              {draft.base_url && (
                <div className="dim3 mono" style={{ fontSize: 10, marginTop: 3 }}>
                  {t('providers.preview')}: {previewUrl} · {t('providers.autoUrl')}
                </div>
              )}

              {/* ---- 模型目录 ---- */}
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '14px 0 6px' }}>
                <label className="dim3" style={{ fontSize: 12, flex: 1 }}>{t('providers.models')}</label>
                <button className="btn" style={{ fontSize: 12 }} disabled={busy} onClick={probeAndMerge}>
                  <Icon name="refresh" size={10} /> {t('providers.fetch')}
                </button>
                <button
                  className="btn" style={{ fontSize: 12 }}
                  onClick={() => setEditModel(emptyModel())}
                >
                  <Icon name="plus" size={10} /> {t('providers.addModel')}
                </button>
              </div>
              {groups.map(([g, ms]) => (
                <div key={g} style={{ marginBottom: 4 }}>
                  <div
                    className="dim3" style={{ fontSize: 12, padding: '4px 2px', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4 }}
                    onClick={() => setCollapsed((c) => ({ ...c, [g]: !c[g] }))}
                  >
                    <Icon name={collapsed[g] ? 'chevron-right' : 'chevron-down'} size={10} /> {g}
                  </div>
                  {!collapsed[g] && ms.map((m) => (
                    <div key={m.id} className="panel" style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '6px 10px', marginBottom: 3 }}>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div className="mono" style={{ fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {m.name || m.id}
                        </div>
                      </div>
                      {m.caps.map((c) => (
                        <span key={c} title={t(`providers.cap_${c}`)} style={{ color: 'var(--text-2)', display: 'inline-flex' }}>
                          <Icon name={CAP_ICONS[c] ?? 'bolt'} size={11} />
                        </span>
                      ))}
                      {/* 票 02：窗口/输出元数据可见性——未识别挂警告标，撞限闸回落 120k 要让人知道 */}
                      {m.context_window != null ? (
                        <span className="dim3" title={t('providers.modelWindow')} style={{ fontSize: 10, fontVariantNumeric: 'tabular-nums' }}>{fmtWindow(m.context_window)}</span>
                      ) : (
                        <span className="chip warn" style={{ fontSize: 10 }} title={t('providers.windowUnknownHint')}>{t('providers.windowUnknown')}</span>
                      )}
                      {m.max_output != null && (
                        <span className="dim3" title={t('providers.modelMaxOut')} style={{ fontSize: 10, fontVariantNumeric: 'tabular-nums' }}>↓{fmtWindow(m.max_output)}</span>
                      )}
                      <button className="btn" style={{ fontSize: 10, padding: '1px 6px' }} onClick={() => setEditModel({ ...m })}>
                        <Icon name="edit" size={10} />
                      </button>
                      <button className="btn" style={{ fontSize: 10, padding: '1px 6px', color: 'var(--err)' }} onClick={() => removeModel(m.id)}>
                        <Icon name="close" size={10} />
                      </button>
                    </div>
                  ))}
                </div>
              ))}
              {draft.models.length === 0 && (
                <div className="dim3" style={{ fontSize: 12, padding: '8px 0' }}>{t('providers.noModels')}</div>
              )}

              {/* ---- 编辑模型内联框 ---- */}
              {editModel && (
                <div className="panel" style={{ marginTop: 8, padding: '10px 12px', outline: '1px solid var(--accent)' }}>
                  <div style={{ display: 'grid', gridTemplateColumns: '76px 1fr', gap: '6px 8px', alignItems: 'center', fontSize: 12 }}>
                    <label className="dim3">{t('providers.modelId')}</label>
                    <input className="input mono" value={editModel.id} onChange={(e) => setEditModel({ ...editModel, id: e.target.value })} />
                    <label className="dim3">{t('providers.modelName')}</label>
                    <input className="input" value={editModel.name ?? ''} onChange={(e) => setEditModel({ ...editModel, name: e.target.value })} />
                    <label className="dim3">{t('providers.modelGroup')}</label>
                    <input className="input" value={editModel.group ?? ''} onChange={(e) => setEditModel({ ...editModel, group: e.target.value })} />
                    <label className="dim3">{t('providers.caps')}</label>
                    <div style={{ display: 'flex', gap: 10 }}>
                      {CAPS.map((c) => (
                        <label key={c} style={{ display: 'flex', gap: 4, alignItems: 'center', fontSize: 12 }}>
                          <input
                            type="checkbox"
                            checked={editModel.caps.includes(c)}
                            onChange={(e) =>
                              setEditModel({
                                ...editModel,
                                caps: e.target.checked ? [...editModel.caps, c] : editModel.caps.filter((x) => x !== c),
                              })
                            }
                          />
                          <Icon name={CAP_ICONS[c]} size={10} /> {t(`providers.cap_${c}`)}
                        </label>
                      ))}
                    </div>
                    {/* 票 02：撞限闸/输出上限的模型元数据（ADR 0068），留空走回落 */}
                    <label className="dim3">{t('providers.modelWindow')}</label>
                    <input
                      className="input mono" type="number" min={0} step={1024}
                      value={editModel.context_window ?? ''}
                      onChange={(e) => setEditModel({ ...editModel, context_window: e.target.value === '' ? null : Number(e.target.value) })}
                    />
                    <label className="dim3">{t('providers.modelMaxOut')}</label>
                    <input
                      className="input mono" type="number" min={0} step={1024}
                      value={editModel.max_output ?? ''}
                      onChange={(e) => setEditModel({ ...editModel, max_output: e.target.value === '' ? null : Number(e.target.value) })}
                    />
                  </div>
                  <div className="dim3" style={{ fontSize: 11, marginTop: 6 }}>{t('providers.metaHint')}</div>
                  <div style={{ display: 'flex', gap: 8, marginTop: 10 }}>
                    <button className="btn primary" disabled={!editModel.id.trim()} onClick={() => saveModel(editModel)}>{t('providers.save')}</button>
                    <button className="btn" onClick={() => setEditModel(null)}>{t('providers.cancel')}</button>
                  </div>
                </div>
              )}

              {err && <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 8 }}>{err}</div>}
              <div style={{ display: 'flex', gap: 8, marginTop: 14 }}>
                <button
                  className="btn primary" disabled={busy || !draft.name.trim() || !draft.base_url.trim()}
                  onClick={() => save(secret.trim() || undefined)}
                >
                  {t('providers.save')}
                </button>
                {!selConfigured && <span className="dim3" style={{ fontSize: 12, alignSelf: 'center' }}>{t('providers.unsaved')}</span>}
              </div>
            </div>
          )}
        </div>
      </div>

      {/* ---- 槽位分配：角色模型槽 → 供应商+模型（Cherry「默认模型」位） ---- */}
      <div className="panel" style={{ marginTop: 14, padding: '12px 14px' }}>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 10, marginBottom: 2 }}>
          <div style={{ fontWeight: 560, fontSize: 12 }}>{t('providers.slots')}</div>
          {/* 票 10：两步任务联动——绑不了常因上游供应商缺 key，汇总行把因果摆上台面 */}
          <div className="dim3" style={{ fontSize: 12 }}>
            {t('providers.slotsSummary', { bound: slotStats.bound, total: slotStats.total })}
            {slotStats.missingKey > 0 && ` · ${t('providers.slotsMissingKey', { count: slotStats.missingKey })}`}
          </div>
        </div>
        <div className="dim3" style={{ fontSize: 12, marginBottom: 8 }}>{t('providers.slotsHint')}</div>
        {slotNames.map((slot) => {
          const b = doc.slots[slot]
          const bp = b ? doc.providers.find((p) => p.id === b.provider_id) : undefined
          const ready = !!(b && bp?.enabled && bp.key_set)
          return (
            <SlotRow
              key={slot}
              slot={slot}
              binding={b}
              providers={doc.providers}
              ready={ready}
              onBind={bind}
              onUnbind={async (s) => { await api.removeSlotBinding(s).catch(() => {}); await reload() }}
            />
          )
        })}
      </div>
    </div>
  )
}

/// 一个槽位的绑定行：供应商下拉 + 模型输入（datalist 供选）+ 状态。
function SlotRow({
  slot, binding, providers, ready, onBind, onUnbind,
}: {
  slot: string
  binding?: { provider_id: string; model: string }
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
  const label = isDedicatedSlot(slot)
    ? `${t('agent.dedicatedTag')} · ${team.find((m) => m.id === slot.slice(DEDICATED_PREFIX.length))?.role ?? slot}`
    : slot
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '6px 0', borderBottom: '1px solid var(--border)' }}>
      <code style={{ fontSize: 12, minWidth: 110, flexShrink: 0 }}>{label}</code>
      <select className="input" style={{ flex: '0 1 190px', width: 'auto' }} value={pid} onChange={(e) => { setPid(e.target.value); setModel('') }}>
        <option value="">{t('providers.pickProvider')}</option>
        {enabled.map((p) => (
          <option key={p.id} value={p.id}>{p.name}</option>
        ))}
      </select>
      <input
        className="input mono" style={{ flex: 1 }} list={`models-${slot}`}
        value={model} placeholder={t('providers.pickModel')}
        onChange={(e) => setModel(e.target.value)}
      />
      <datalist id={`models-${slot}`}>
        {provider?.models.map((m) => <option key={m.id} value={m.id} />)}
      </datalist>
      <button
        className="btn" style={{ fontSize: 12 }}
        disabled={!pid || !model.trim()}
        onClick={() => onBind(slot, pid, model.trim())}
      >
        {t('providers.bind')}
      </button>
      {binding && (
        <button className="btn" style={{ fontSize: 12 }} onClick={() => onUnbind(slot)}>{t('providers.unbind')}</button>
      )}
      <span
        className="chip"
        data-slot-state={binding ? 'bound' : slot === 'jev' ? 'empty' : 'default'}
        style={ready ? { color: 'var(--ok)' } : { color: 'var(--err)' }}
      >
        {binding
          ? t('providers.bound')
          : slot === 'jev'
            ? t('providers.jevEmpty')
            : slot === 'default'
              ? t('providers.unbound')
              : t('providers.usingDefault')}
      </span>
    </div>
  )
}
