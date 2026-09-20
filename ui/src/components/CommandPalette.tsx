// ⌘K 命令面板（票 29）：指令/导航/快速打开统一入口。
// 指令项与 composer 文本指令同语义（同 api 命令通道）：
// /stamp /skip /rewind /pause /resume /sleep 走的都是这些 api.*。
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, type ActionId } from '../keymap'

interface Item {
  id: string
  group: 'cmd' | 'nav' | 'open'
  label: string
  hint?: string
  run: () => unknown
}

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
  const { stages, team, artifacts, openTab, setRailOpen, setSplitOpen, setActiveTab, railOpen, splitOpen, tabs, activeTab, closeTab, invalidate } = useUiStore()
  const [q, setQ] = useState('')
  const [sel, setSel] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const items = useMemo<Item[]>(() => {
    const bind = (a: ActionId) => formatBinding(bindingFor(a))
    const list: Item[] = [
      // 指令：与 composer /verb 同一命令通道
      { id: 'c-stamp', group: 'cmd', label: t('palette.stamp'), run: () => api.stamp() },
      { id: 'c-skip', group: 'cmd', label: t('palette.skip'), run: () => api.skip() },
      {
        id: 'c-rewind', group: 'cmd', label: t('palette.rewindPrev'),
        run: () => {
          const active = [...stages].reverse().find((s) => s.state === 'active' || s.state === 'waiting_stamp')
          if (active && active.seq > 0) return api.rewind(active.seq - 1)
        },
      },
      { id: 'c-pause', group: 'cmd', label: t('palette.pause'), run: () => api.pause() },
      { id: 'c-resume', group: 'cmd', label: t('palette.resume'), run: () => api.resume() },
      { id: 'c-checks', group: 'cmd', label: t('palette.checks'), run: () => api.runChecks() },
      { id: 'c-advance', group: 'cmd', label: t('palette.advance'), run: () => api.advance() },
      { id: 'c-sleep', group: 'cmd', label: t('palette.sleepAll'), run: () => api.sleepAll() },
      // 导航
      { id: 'n-rail', group: 'nav', label: t('palette.rail'), hint: bind('toggleRail'), run: () => setRailOpen(!railOpen) },
      { id: 'n-split', group: 'nav', label: t('palette.split'), hint: bind('splitEditor'), run: () => setSplitOpen(!splitOpen) },
      { id: 'n-timeline', group: 'nav', label: t('palette.timeline'), run: () => setActiveTab('timeline') },
      {
        id: 'n-close', group: 'nav', label: t('palette.closeTab'), hint: bind('closeTab'),
        run: () => closeTab(activeTab),
      },
      // 快速打开
      ...team.map((m) => ({
        id: `o-a-${m.id}`, group: 'open' as const,
        label: `${t('palette.openAgent')} ${m.role}`,
        run: () => openTab({ id: `agent:${m.id}`, kind: 'agent', title: m.role, agentId: m.id, role: m.role }),
      })),
      ...artifacts.map((a) => ({
        id: `o-f-${a.path}-v${a.version}`, group: 'open' as const,
        label: `${t('palette.openArtifact')} ${a.path} v${a.version}`,
        run: () => openTab({ id: `art:${a.path}`, kind: 'artifact', title: a.path, path: a.path }),
      })),
    ]
    void tabs
    return list
  }, [t, stages, team, artifacts, openTab, setRailOpen, setSplitOpen, setActiveTab, railOpen, splitOpen, closeTab, activeTab, tabs])

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase()
    if (!needle) return items
    return items.filter((i) => i.label.toLowerCase().includes(needle))
  }, [items, q])

  const runItem = async (i: Item) => {
    onClose()
    await i.run()
    await invalidate()
  }

  const groups: { key: Item['group']; title: string }[] = [
    { key: 'cmd', title: t('palette.commands') },
    { key: 'nav', title: t('palette.nav') },
    { key: 'open', title: t('palette.open') },
  ]

  let flatIdx = -1
  return (
    <div
      style={{ position: 'fixed', inset: 0, zIndex: 70, background: 'rgba(0,0,0,.35)', display: 'flex', justifyContent: 'center', paddingTop: '12vh' }}
      onClick={onClose}
    >
      <div
        className="panel"
        style={{ width: 480, maxHeight: '60vh', display: 'flex', flexDirection: 'column', overflow: 'hidden', boxShadow: '0 12px 40px rgba(0,0,0,.4)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <input
          ref={inputRef}
          value={q}
          placeholder={t('palette.placeholder')}
          onChange={(e) => { setQ(e.target.value); setSel(0) }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') { e.preventDefault(); onClose(); return }
            if (e.key === 'ArrowDown') { e.preventDefault(); setSel((sel + 1) % Math.max(filtered.length, 1)); return }
            if (e.key === 'ArrowUp') { e.preventDefault(); setSel((sel - 1 + filtered.length) % Math.max(filtered.length, 1)); return }
            if (e.key === 'Enter' && filtered[sel]) { e.preventDefault(); void runItem(filtered[sel]) }
          }}
          style={{ padding: '12px 14px', fontSize: 14, background: 'transparent', border: 'none', borderBottom: '1px solid var(--border)', outline: 'none' }}
        />
        <div style={{ flex: 1, overflowY: 'auto', padding: '6px' }}>
          {filtered.length === 0 && (
            <div className="dim3" style={{ padding: '14px', fontSize: 12 }}>{t('palette.empty')}</div>
          )}
          {groups.map((g) => {
            const rows = filtered.filter((i) => i.group === g.key)
            if (!rows.length) return null
            return (
              <div key={g.key}>
                <div className="dim3" style={{ fontSize: 10, padding: '6px 8px 2px', textTransform: 'uppercase', letterSpacing: '.06em' }}>
                  {g.title}
                </div>
                {rows.map((i) => {
                  flatIdx += 1
                  const idx = flatIdx
                  return (
                    <div
                      key={i.id}
                      onClick={() => void runItem(i)}
                      onMouseEnter={() => setSel(idx)}
                      style={{
                        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                        padding: '7px 10px', borderRadius: 6, cursor: 'pointer', fontSize: 13,
                        background: idx === sel ? 'var(--bg-2)' : 'transparent',
                      }}
                    >
                      <span>{i.label}</span>
                      {i.hint && <span className="dim3 mono" style={{ fontSize: 11 }}>{i.hint}</span>}
                    </div>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
