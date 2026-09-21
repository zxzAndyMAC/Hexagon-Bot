// 名单字段勾选器（.scratch/settings-density 02，owner 裁决 2026-09）：
// 技能/MCP 授权等名单曾是逗号分隔文本输入——owner 裁决「技能不应该是输入框，
// 做成添加按钮 → 弹窗搜索+勾选+添加」。已选项渲染为 chip 行（× 移除）。
// 弹层外壳复用 ConfirmDialog 模式（fixed 遮罩 + Esc/点遮罩关 + --popover 不透明底）。
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'

export type PickSource = 'skills' | 'mcp'
type Option = { id: string; desc: string; badge: string; badgeOk: boolean }

// 数据源惰性加载——picker 打开才发 IPC，避免每个编辑器挂载都打一次清单。
async function loadOptions(source: PickSource): Promise<Option[]> {
  if (source === 'skills') {
    const rows = await api.listSkills() // projectless 安全：回落全局层
    return rows.map((r) => ({
      id: r.name, desc: r.description, badge: r.origin, badgeOk: r.origin !== 'builtin',
    }))
  }
  const rows = await api.mcpEntries()
  return rows.map((r) => ({
    id: r.name,
    desc: r.transport === 'remote' ? (r.url ?? '') : [r.command, ...r.args].join(' '),
    badge: r.origin, badgeOk: r.origin === 'global',
  }))
}

/** chip 行字段：已选值 + 「+ 添加」开勾选弹窗。value/onChange 走字符串数组，
 *  调用方若仍存逗号串自行 split/join（保持旧持久化面不动）。 */
export function EntityChips({
  value,
  onChange,
  source,
}: {
  value: string[]
  onChange: (ids: string[]) => void
  source: PickSource
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  return (
    <>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
        {value.map((v) => (
          <span key={v} className="chip mono" style={{ gap: 5, paddingRight: 5 }}>
            {v}
            <button
              type="button"
              aria-label={t('picker.remove')}
              title={t('picker.remove')}
              onClick={() => onChange(value.filter((x) => x !== v))}
              style={{ all: 'unset', cursor: 'pointer', lineHeight: 1, opacity: 0.65, fontSize: 12 }}
            >
              ×
            </button>
          </span>
        ))}
        <button type="button" className="btn" onClick={() => setOpen(true)}>
          {t('picker.add')}
        </button>
      </div>
      {open && (
        <EntityPicker
          source={source}
          selected={value}
          onClose={() => setOpen(false)}
          onConfirm={(ids) => { onChange(ids); setOpen(false) }}
        />
      )}
    </>
  )
}

function EntityPicker({
  source,
  selected,
  onConfirm,
  onClose,
}: {
  source: PickSource
  selected: string[]
  onConfirm: (ids: string[]) => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [opts, setOpts] = useState<Option[] | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [q, setQ] = useState('')
  const [sel, setSel] = useState<Set<string>>(new Set(selected))
  const searchRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    searchRef.current?.focus()
    let live = true
    loadOptions(source)
      .then((o) => { if (live) setOpts(o) })
      .catch((e) => { if (live) setErr(errText(e)) })
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); onClose() } }
    window.addEventListener('keydown', h, true)
    return () => { live = false; window.removeEventListener('keydown', h, true) }
  }, [source, onClose])

  const shown = useMemo(() => {
    const base = opts ?? []
    // 已选但不在清单里的名字（手填的历史值/已删除实体）合成行保留——
    // 否则用户看不见也取不消它，只能整字段重写。
    const missing = selected
      .filter((id) => !base.some((o) => o.id === id))
      .map((id) => ({ id, desc: '', badge: 'custom', badgeOk: false }))
    const ql = q.trim().toLowerCase()
    return [...missing, ...base].filter((o) =>
      !ql || o.id.toLowerCase().includes(ql) || o.desc.toLowerCase().includes(ql),
    )
  }, [opts, q, selected])

  const toggle = (id: string) =>
    setSel((s) => {
      const n = new Set(s)
      if (n.has(id)) n.delete(id)
      else n.add(id)
      return n
    })

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 80,
        background: 'rgba(0,0,0,.45)', display: 'flex',
        alignItems: 'center', justifyContent: 'center',
      }}
      onClick={onClose}
      role="presentation"
    >
      <div
        className="panel panel-float"
        role="dialog"
        aria-modal="true"
        aria-label={t(`picker.title_${source}`)}
        style={{
          width: 'min(520px, 92vw)', padding: 14, display: 'flex', flexDirection: 'column',
          boxShadow: '0 12px 40px rgba(0,0,0,.4)',
          maxHeight: '70vh',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ fontWeight: 560, fontSize: 14, marginBottom: 10 }}>
          {t(`picker.title_${source}`)}
        </div>
        <input
          ref={searchRef}
          className="input"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder={t('picker.search')}
          style={{ width: '100%', boxSizing: 'border-box' }}
        />
        <div style={{ flex: 1, overflowY: 'auto', marginTop: 8, minHeight: 120 }}>
          {err && <div style={{ fontSize: 12, color: 'var(--err)', padding: '8px 2px' }}>{err}</div>}
          {!err && opts === null && <div className="dim3" style={{ fontSize: 12, padding: '12px 2px' }}>…</div>}
          {!err && opts !== null && shown.length === 0 && (
            <div className="dim3" style={{ fontSize: 12, padding: '12px 2px' }}>{t('picker.empty')}</div>
          )}
          {shown.map((o) => (
            <label
              key={o.id}
              style={{
                display: 'flex', alignItems: 'center', gap: 9, padding: '7px 6px',
                borderRadius: 7, cursor: 'pointer',
              }}
            >
              <input type="checkbox" checked={sel.has(o.id)} onChange={() => toggle(o.id)} />
              <span className="mono" style={{ fontSize: 12, flexShrink: 0 }}>{o.id}</span>
              <span className={`chip ${o.badgeOk ? 'ok' : ''}`}>{o.badge}</span>
              <span className="dim3" style={{ flex: 1, minWidth: 0, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                {o.desc}
              </span>
            </label>
          ))}
        </div>
        <div style={{ display: 'flex', gap: 8, marginTop: 12, justifyContent: 'flex-end', alignItems: 'center' }}>
          <span className="dim3" style={{ marginRight: 'auto', fontSize: 12 }}>
            {t('picker.count', { n: sel.size })}
          </span>
          <button className="btn" onClick={onClose}>{t('agent.cancel')}</button>
          <button className="btn primary" onClick={() => onConfirm([...sel])}>{t('picker.done')}</button>
        </div>
      </div>
    </div>
  )
}
