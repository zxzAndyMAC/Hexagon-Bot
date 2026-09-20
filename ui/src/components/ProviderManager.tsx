// 供应商管理（共享件）：设置页「模型与凭据」分区与启动页设置同用。
// 布局对齐 Cherry Studio：左列=内置常见供应商+已配置供应商+底部固定「添加」；
// 右列=选中供应商详情（密钥/端点/模型目录）——检测、拉取模型列表、能力标记、
// 编辑模型一应俱全；底部「槽位分配」把角色模型槽绑到 供应商+模型。
// 数据语义：非密配置存 providers.json；key 只写 keychain（provider/<id>）。
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type ModelEntry, type ProviderDef, type ProviderDoc, type RoleDef } from '../api'
import { Icon, type IconName } from './Icon'

/// 内置常见供应商目录（未配置时灰显在左列，点选即填右栏默认值）。
const PRESETS: { name: string; kind: 'openai' | 'anthropic'; base_url: string }[] = [
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

const slug = (s: string) =>
  s.trim().toLowerCase().replace(/[^a-z0-9\u4e00-\u9fff]+/g, '-').replace(/^-+|-+$/g, '') || `p${Date.now()}`

function emptyDef(): ProviderDef {
  return { id: '', name: '', kind: 'openai', base_url: '', models: [], enabled: true, key_set: false }
}

export function ProviderManager() {
  const { t } = useTranslation()
  const [doc, setDoc] = useState<ProviderDoc>({ providers: [], slots: {} })
  const [roles, setRoles] = useState<RoleDef[]>([])
  const [sel, setSel] = useState<string>('') // provider id / 'preset:<name>' / 'new'
  const [draft, setDraft] = useState<ProviderDef | null>(null)
  const [secret, setSecret] = useState('')
  const [showKey, setShowKey] = useState(false)
  const [probe, setProbe] = useState<{ ok: boolean; text: string } | null>(null)
  const [editModel, setEditModel] = useState<ModelEntry | null>(null)
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
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
      setDraft(p ? { ...emptyDef(), name: p.name, kind: p.kind, base_url: p.base_url } : null)
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
      setSecret('')
      await reload(def.id)
    } catch (e) {
      setErr(errText(e))
    } finally {
      setBusy(false)
    }
  }

  const toggleEnabled = async (p: ProviderDef) => {
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
        return old ? { ...m, caps: old.caps.length ? old.caps : m.caps, name: old.name } : m
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
    Object.keys(doc.slots).forEach((k) => s.add(k))
    return [...s].sort()
  }, [roles, doc.slots])

  const bind = async (slot: string, providerId: string, model: string) => {
    await api.setSlotBinding(slot, providerId, model).catch((e) => setErr(errText(e)))
    await reload()
  }

  const previewUrl = draft
    ? `${draft.base_url.replace(/\/+$/, '')}${draft.kind === 'anthropic' ? '/v1/messages' : '/chat/completions'}`
    : ''

  const row: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 8, padding: '8px 10px', borderRadius: 8, cursor: 'pointer' }

  return (
    <div>
      <div className="dim3" style={{ fontSize: 11, marginBottom: 10 }}>{t('providers.hint')}</div>
      <div style={{ display: 'flex', gap: 14, alignItems: 'flex-start' }}>
        {/* ---- 左列：供应商列表（Cherry 式） ---- */}
        <div
          className="panel"
          style={{ width: 190, flexShrink: 0, display: 'flex', flexDirection: 'column', maxHeight: 420 }}
        >
          <div style={{ flex: 1, overflowY: 'auto', padding: '6px 4px' }}>
            {doc.providers.map((p) => (
              <div
                key={p.id}
                style={{ ...row, outline: sel === p.id ? '1px solid var(--accent)' : undefined }}
                onClick={() => pick(p.id)}
              >
                <div style={{ flex: 1, minWidth: 0, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {p.name}
                </div>
                <button
                  className={`btn ${p.enabled ? 'primary' : ''}`}
                  style={{ fontSize: 10, padding: '1px 7px' }}
                  title={t('providers.enabled')}
                  onClick={(e) => { e.stopPropagation(); toggleEnabled(p) }}
                >
                  {p.enabled ? 'ON' : 'OFF'}
                </button>
              </div>
            ))}
            {unconfiguredPresets.length > 0 && doc.providers.length > 0 && (
              <div style={{ borderTop: '1px solid var(--border)', margin: '4px 6px' }} />
            )}
            {unconfiguredPresets.map((p) => (
              <div
                key={p.name}
                className="dim3"
                style={{ ...row, outline: sel === `preset:${p.name}` ? '1px solid var(--accent)' : undefined }}
                onClick={() => pick(`preset:${p.name}`)}
              >
                <div style={{ flex: 1, fontSize: 12 }}>{p.name}</div>
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

        {/* ---- 右列：供应商详情 ---- */}
        <div style={{ flex: 1, minWidth: 0 }}>
          {!draft && <div className="dim3" style={{ fontSize: 12, padding: '30px 0', textAlign: 'center' }}>{t('providers.pickHint')}</div>}
          {draft && (
            <>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 12 }}>
                <input
                  className="btn" style={{ fontWeight: 560, flex: '0 1 200px' }}
                  value={draft.name} placeholder={t('providers.name')}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                />
                <span className="chip">{draft.kind}</span>
                <span className="chip" style={draft.key_set ? { color: 'var(--ok)' } : { color: 'var(--err)' }}>
                  {draft.key_set ? t('providers.keySet') : t('providers.keyMissing')}
                </span>
                <div style={{ flex: 1 }} />
                {selConfigured && (
                  <button className="btn" style={{ fontSize: 11, color: 'var(--err)' }} onClick={del}>{t('providers.delete')}</button>
                )}
              </div>

              <label className="dim3" style={{ fontSize: 11 }}>{t('providers.key')}</label>
              <div style={{ display: 'flex', gap: 6, margin: '4px 0 10px' }}>
                <input
                  className="btn mono" style={{ flex: 1 }}
                  type={showKey ? 'text' : 'password'}
                  value={secret}
                  placeholder={draft.key_set ? t('providers.keyKeep') : 'sk-…'}
                  onChange={(e) => setSecret(e.target.value)}
                />
                <button className="btn" title={t('providers.showKey')} onClick={() => setShowKey((s) => !s)}>
                  <Icon name="vision" size={12} />
                </button>
                <button className="btn" disabled={busy} onClick={probeAndMerge}>{t('providers.check')}</button>
              </div>
              {probe && (
                <div style={{ fontSize: 11, marginBottom: 8, color: probe.ok ? 'var(--ok)' : 'var(--err)' }}>{probe.text}</div>
              )}

              <label className="dim3" style={{ fontSize: 11 }}>{t('providers.kind')}</label>
              <select
                className="btn" style={{ margin: '4px 0 10px', display: 'block' }}
                value={draft.kind}
                onChange={(e) => setDraft({ ...draft, kind: e.target.value as ProviderDef['kind'] })}
              >
                <option value="openai">OpenAI 兼容</option>
                <option value="anthropic">Anthropic</option>
              </select>

              <label className="dim3" style={{ fontSize: 11 }}>{t('providers.baseUrl')}</label>
              <input
                className="btn mono" style={{ width: '100%', marginTop: 4 }}
                value={draft.base_url} placeholder="https://…/v1"
                onChange={(e) => setDraft({ ...draft, base_url: e.target.value })}
              />
              {draft.base_url && (
                <div className="dim3 mono" style={{ fontSize: 10, marginTop: 3 }}>{t('providers.preview')}: {previewUrl}</div>
              )}

              {/* ---- 模型目录 ---- */}
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, margin: '14px 0 6px' }}>
                <label className="dim3" style={{ fontSize: 11, flex: 1 }}>{t('providers.models')}</label>
                <button className="btn" style={{ fontSize: 11 }} disabled={busy} onClick={probeAndMerge}>
                  <Icon name="refresh" size={10} /> {t('providers.fetch')}
                </button>
                <button
                  className="btn" style={{ fontSize: 11 }}
                  onClick={() => setEditModel({ id: '', name: '', group: '', caps: [] })}
                >
                  <Icon name="plus" size={10} /> {t('providers.addModel')}
                </button>
              </div>
              {groups.map(([g, ms]) => (
                <div key={g} style={{ marginBottom: 4 }}>
                  <div
                    className="dim3" style={{ fontSize: 11, padding: '4px 2px', cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 4 }}
                    onClick={() => setCollapsed((c) => ({ ...c, [g]: !c[g] }))}
                  >
                    <Icon name={collapsed[g] ? 'chevron-right' : 'chevron-down'} size={10} /> {g}
                  </div>
                  {!collapsed[g] && ms.map((m) => (
                    <div key={m.id} className="panel" style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '6px 10px', marginBottom: 3 }}>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div className="mono" style={{ fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {m.name || m.id}
                        </div>
                      </div>
                      {m.caps.map((c) => (
                        <span key={c} title={t(`providers.cap_${c}`)} style={{ color: 'var(--text-2)', display: 'inline-flex' }}>
                          <Icon name={CAP_ICONS[c] ?? 'bolt'} size={11} />
                        </span>
                      ))}
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
                <div className="dim3" style={{ fontSize: 11, padding: '8px 0' }}>{t('providers.noModels')}</div>
              )}

              {/* ---- 编辑模型内联框 ---- */}
              {editModel && (
                <div className="panel" style={{ marginTop: 8, padding: '10px 12px', outline: '1px solid var(--accent)' }}>
                  <div style={{ display: 'grid', gridTemplateColumns: '76px 1fr', gap: '6px 8px', alignItems: 'center', fontSize: 12 }}>
                    <label className="dim3">{t('providers.modelId')}</label>
                    <input className="btn mono" value={editModel.id} onChange={(e) => setEditModel({ ...editModel, id: e.target.value })} />
                    <label className="dim3">{t('providers.modelName')}</label>
                    <input className="btn" value={editModel.name ?? ''} onChange={(e) => setEditModel({ ...editModel, name: e.target.value })} />
                    <label className="dim3">{t('providers.modelGroup')}</label>
                    <input className="btn" value={editModel.group ?? ''} onChange={(e) => setEditModel({ ...editModel, group: e.target.value })} />
                    <label className="dim3">{t('providers.caps')}</label>
                    <div style={{ display: 'flex', gap: 10 }}>
                      {CAPS.map((c) => (
                        <label key={c} style={{ display: 'flex', gap: 4, alignItems: 'center', fontSize: 11 }}>
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
                  </div>
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
                {!selConfigured && <span className="dim3" style={{ fontSize: 11, alignSelf: 'center' }}>{t('providers.unsaved')}</span>}
              </div>
            </>
          )}
        </div>
      </div>

      {/* ---- 槽位分配：角色模型槽 → 供应商+模型（Cherry「默认模型」位） ---- */}
      <div style={{ borderTop: '1px solid var(--border)', marginTop: 16, paddingTop: 12 }}>
        <div style={{ fontWeight: 560, fontSize: 12, marginBottom: 2 }}>{t('providers.slots')}</div>
        <div className="dim3" style={{ fontSize: 11, marginBottom: 8 }}>{t('providers.slotsHint')}</div>
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
  providers: ProviderDef[]
  ready: boolean
  onBind: (slot: string, providerId: string, model: string) => void
  onUnbind: (slot: string) => void
}) {
  const { t } = useTranslation()
  const [pid, setPid] = useState(binding?.provider_id ?? '')
  const [model, setModel] = useState(binding?.model ?? '')
  const provider = providers.find((p) => p.id === pid)
  const enabled = providers.filter((p) => p.enabled)
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '6px 0', borderBottom: '1px solid var(--border)' }}>
      <code style={{ fontSize: 11, width: 110, flexShrink: 0 }}>{slot}</code>
      <select className="btn" style={{ flex: '0 1 170px' }} value={pid} onChange={(e) => { setPid(e.target.value); setModel('') }}>
        <option value="">{t('providers.pickProvider')}</option>
        {enabled.map((p) => (
          <option key={p.id} value={p.id}>{p.name}</option>
        ))}
      </select>
      <input
        className="btn mono" style={{ flex: 1 }} list={`models-${slot}`}
        value={model} placeholder={t('providers.pickModel')}
        onChange={(e) => setModel(e.target.value)}
      />
      <datalist id={`models-${slot}`}>
        {provider?.models.map((m) => <option key={m.id} value={m.id} />)}
      </datalist>
      <button
        className="btn" style={{ fontSize: 11 }}
        disabled={!pid || !model.trim()}
        onClick={() => onBind(slot, pid, model.trim())}
      >
        {t('providers.bind')}
      </button>
      {binding && (
        <button className="btn" style={{ fontSize: 11 }} onClick={() => onUnbind(slot)}>{t('providers.unbind')}</button>
      )}
      <span className="chip" style={ready ? { color: 'var(--ok)' } : { color: 'var(--err)' }}>
        {ready ? t('providers.bound') : t('providers.unbound')}
      </span>
    </div>
  )
}
