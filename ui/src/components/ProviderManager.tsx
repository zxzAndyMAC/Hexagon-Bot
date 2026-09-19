// 供应商管理（共享件）：设置页「模型与凭据」分区与启动页设置层同用。
// 非密配置存 ~/.config/hexagon/providers.json；key 只写 keychain 永不回读——
// 「留空 = 不改」语义；default 槽是兜底配置（角色槽缺省时走它）。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, type ProviderCfg } from '../api'
import { Icon } from './Icon'

interface Form {
  slot: string
  kind: 'anthropic' | 'openai'
  base_url: string
  model: string
  secret: string
}

const EMPTY: Form = { slot: '', kind: 'openai', base_url: '', model: '', secret: '' }

/// kind 切换时给端点兜底默认（用户已自填则不覆盖）。
const KIND_DEFAULT_URL: Record<Form['kind'], string> = {
  openai: 'https://api.openai.com/v1',
  anthropic: 'https://api.anthropic.com',
}

export function ProviderManager() {
  const { t } = useTranslation()
  const [rows, setRows] = useState<ProviderCfg[]>([])
  const [form, setForm] = useState<Form | null>(null)
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  const reload = () => api.listProviders().then(setRows).catch(() => {})
  useEffect(() => { reload() }, [])

  const set = (p: Partial<Form>) => setForm((f) => (f ? { ...f, ...p } : f))

  const save = async () => {
    if (!form) return
    setBusy(true)
    setErr('')
    try {
      await api.saveProvider(
        form.slot.trim(), form.kind, form.base_url.trim(), form.model.trim(),
        form.secret.trim() || undefined,
      )
      setForm(null)
      await reload()
    } catch (e) {
      setErr(String(e))
    } finally {
      setBusy(false)
    }
  }

  const del = async (slot: string) => {
    await api.deleteProvider(slot).catch((e) => setErr(String(e)))
    await reload()
  }

  const kindUrl = form ? KIND_DEFAULT_URL[form.kind] : ''

  return (
    <div>
      <div className="dim3" style={{ fontSize: 11, marginBottom: 10 }}>{t('providers.hint')}</div>
      {rows.map((r) => (
        <div
          key={r.slot}
          style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 0', borderBottom: '1px solid var(--border)' }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <code style={{ fontSize: 12, fontWeight: 560 }}>{r.slot}</code>
              <span className="chip">{r.kind}</span>
              <span className="chip" style={r.key_set ? { color: 'var(--ok)' } : { color: 'var(--err)' }}>
                {r.key_set ? t('providers.keySet') : t('providers.keyMissing')}
              </span>
            </div>
            <div className="dim3 mono" style={{ fontSize: 11, marginTop: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {r.model} · {r.base_url}
            </div>
          </div>
          <button
            className="btn" style={{ fontSize: 11, display: 'inline-flex', alignItems: 'center', gap: 4 }}
            onClick={() => setForm({ slot: r.slot, kind: r.kind, base_url: r.base_url, model: r.model, secret: '' })}
          >
            <Icon name="edit" size={11} /> {t('providers.edit')}
          </button>
          <button className="btn" style={{ fontSize: 11, color: 'var(--err)' }} onClick={() => del(r.slot)}>
            {t('providers.delete')}
          </button>
        </div>
      ))}
      {rows.length === 0 && !form && (
        <div className="dim3" style={{ fontSize: 12, padding: '14px 0' }}>{t('providers.empty')}</div>
      )}

      {form ? (
        <div className="panel" style={{ marginTop: 12, padding: '12px 14px' }}>
          <div style={{ display: 'grid', gridTemplateColumns: '110px 1fr', gap: '8px 10px', alignItems: 'center', fontSize: 12 }}>
            <label className="dim3">{t('providers.slot')}</label>
            <input className="btn mono" value={form.slot} placeholder="chat / default / …" onChange={(e) => set({ slot: e.target.value })} />
            <label className="dim3">{t('providers.kind')}</label>
            <select
              className="btn"
              value={form.kind}
              onChange={(e) => {
                const kind = e.target.value as Form['kind']
                set({ kind, base_url: form.base_url || KIND_DEFAULT_URL[kind] })
              }}
            >
              <option value="openai">OpenAI 兼容</option>
              <option value="anthropic">Anthropic</option>
            </select>
            <label className="dim3">{t('providers.baseUrl')}</label>
            <input className="btn mono" value={form.base_url} placeholder={kindUrl} onChange={(e) => set({ base_url: e.target.value })} />
            <label className="dim3">{t('providers.model')}</label>
            <input className="btn mono" value={form.model} placeholder="gpt-4o / claude-sonnet-4-6 / deepseek-chat …" onChange={(e) => set({ model: e.target.value })} />
            <label className="dim3">{t('providers.key')}</label>
            <input
              className="btn mono" type="password" value={form.secret}
              placeholder={rows.find((r) => r.slot === form.slot)?.key_set ? t('providers.keyKeep') : 'sk-…'}
              onChange={(e) => set({ secret: e.target.value })}
            />
          </div>
          {err && <div style={{ color: 'var(--err)', fontSize: 12, marginTop: 8 }}>{err}</div>}
          <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
            <button
              className="btn primary"
              disabled={busy || !form.slot.trim() || !form.base_url.trim() || !form.model.trim()}
              onClick={save}
            >
              {t('providers.save')}
            </button>
            <button className="btn" onClick={() => { setForm(null); setErr('') }}>{t('providers.cancel')}</button>
          </div>
        </div>
      ) : (
        <button
          className="btn" style={{ marginTop: 12, fontSize: 12, display: 'inline-flex', alignItems: 'center', gap: 5 }}
          onClick={() => setForm({ ...EMPTY })}
        >
          <Icon name="plus" size={11} /> {t('providers.add')}
        </button>
      )}
    </div>
  )
}
