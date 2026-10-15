// 名单字段勾选器（.scratch/settings-density 02，owner 裁决 2026-09）：
// 技能/MCP 授权等名单曾是逗号分隔文本输入——owner 裁决「技能不应该是输入框，
// 做成添加按钮 → 弹窗搜索+勾选+添加」。已选项渲染为 chip 行（× 移除）。
// 弹层外壳复用 ConfirmDialog 模式（fixed 遮罩 + Esc/点遮罩关 + --surface 不透明底）。
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty } from '@/components/ui/empty'
import { Input } from '@/components/ui/input'
import { Icon } from './Icon'

// ui-redesign 票 11（components E4「技能（chip 行 + 添加，EntityPicker）」；票 10 偏差 7 留给本票）：
// 已选值 = line chip（等宽）+ × 图标钮，「添加」= ghost xs；勾选弹窗换成 A17 模态外观（--scrim 遮罩、overlay 面、圆角 16）。

export type PickSource = 'skills' | 'mcp'
export type Option = { id: string; desc: string; badge: string; badgeOk?: boolean }

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
  options,
  title,
}: {
  value: string[]
  onChange: (ids: string[]) => void
  /** IPC 名单（skills/mcp）。本地名单（如向导阶段角色）走 options 跳过加载 */
  source?: PickSource
  options?: Option[]
  /** 本地名单时的弹窗标题——picker.title_* 只登记了 skills/mcp */
  title?: string
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  return (
    <>
      <div className="flex flex-wrap items-center gap-1.5">
        {value.map((v) => (
          <Badge key={v} variant="line" data-entity-chip={v} className="pr-1 font-mono">
            {v}
            <button
              type="button"
              aria-label={`${t('picker.remove')} ${v}`}
              title={t('picker.remove')}
              onClick={() => onChange(value.filter((x) => x !== v))}
              className="hx-focus grid size-4 cursor-pointer place-items-center rounded-full text-ink-3 live:hover:bg-hover live:hover:text-ink"
            >
              <Icon name="close" size={10} />
            </button>
          </Badge>
        ))}
        <Button size="xs" variant="ghost" onClick={() => setOpen(true)}>
          {t('picker.add')}
        </Button>
      </div>
      {open && (
        <EntityPicker
          source={source}
          options={options}
          title={title}
          selected={value}
          onClose={() => setOpen(false)}
          onConfirm={(ids) => { onChange(ids); setOpen(false) }}
        />
      )}
    </>
  )
}

export function EntityPicker({
  source,
  options,
  title,
  selected,
  onConfirm,
  onClose,
}: {
  source?: PickSource
  options?: Option[]
  title?: string
  selected: string[]
  onConfirm: (ids: string[]) => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  // 本地名单直接做初始值（打开时的快照）；没有才走 IPC
  const [opts, setOpts] = useState<Option[] | null>(options ?? null)
  const [err, setErr] = useState<string | null>(null)
  const [q, setQ] = useState('')
  const [sel, setSel] = useState<Set<string>>(new Set(selected))
  const searchRef = useRef<HTMLInputElement>(null)
  const heading = title ?? (source ? t(`picker.title_${source}`) : '')

  useEffect(() => {
    searchRef.current?.focus()
    let live = true
    if (opts === null && source) {
      loadOptions(source)
        .then((o) => { if (live) setOpts(o) })
        .catch((e) => { if (live) setErr(errText(e)) })
    }
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); onClose() } }
    window.addEventListener('keydown', h, true)
    return () => { live = false; window.removeEventListener('keydown', h, true) }
  }, [source, opts, onClose])

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
    <div className="fixed inset-0 z-[80] flex items-center justify-center bg-scrim" onClick={onClose} role="presentation">
      <div
        role="dialog"
        aria-modal="true"
        aria-label={heading}
        className="flex max-h-[70vh] w-[min(520px,92vw)] flex-col rounded-[16px] bg-surface p-4 shadow-[var(--shadow-overlay)]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-3 text-[15px] font-semibold text-ink">{heading}</div>
        <Input
          ref={searchRef}
          leading={<Icon name="search" size={14} />}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder={t('picker.search')}
          aria-label={t('picker.search')}
        />
        <div className="mt-2 min-h-[120px] flex-1 overflow-y-auto">
          {err && <div role="alert" className="px-0.5 py-2 text-[12.5px] text-red">{err}</div>}
          {!err && opts === null && <div className="px-0.5 py-3 text-[12.5px] text-ink-3">…</div>}
          {!err && opts !== null && shown.length === 0 && (
            <Empty size="sm" icon="search" title={t('picker.empty')} desc={q.trim() ? t('feedback.searchEmptyBody') : t('feedback.pickerEmptyBody')}
              action={q.trim() ? <Button size="sm" onClick={() => setQ('')}>{t('feedback.clearSearch')}</Button> : undefined} />
          )}
          {shown.map((o) => (
            <label key={o.id} className="flex cursor-pointer items-center gap-2.5 rounded-[7px] px-1.5 py-[7px] live:hover:bg-hover">
              <input type="checkbox" className="accent-[var(--accent)]" checked={sel.has(o.id)} onChange={() => toggle(o.id)} />
              <span data-picker-id className="flex-none font-mono text-[12px] text-ink">{o.id}</span>
              {o.badge && <Badge variant={o.badgeOk ? 'ok' : 'neutral'} className="h-[18px] px-1.5 text-[11px]">{o.badge}</Badge>}
              <span className="min-w-0 flex-1 truncate text-[12px] text-ink-3">{o.desc}</span>
            </label>
          ))}
        </div>
        <div className="mt-3 flex items-center justify-end gap-2">
          <span className="mr-auto text-[12px] text-ink-3">{t('picker.count', { n: sel.size })}</span>
          <Button size="sm" variant="quiet" onClick={onClose}>{t('agent.cancel')}</Button>
          <Button size="sm" variant="primary" onClick={() => onConfirm([...sel])}>{t('picker.done')}</Button>
        </div>
      </div>
    </div>
  )
}
