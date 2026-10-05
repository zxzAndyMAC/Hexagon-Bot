// ⌘K 命令面板（票 29）：指令/导航/快速打开统一入口。
// 指令项与 composer 文本指令同语义（同 api 命令通道）：
// /stamp /skip /rewind /pause /resume /sleep 走的都是这些 api.*。
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, type ActionId } from '../keymap'
import { runStageOp } from '../stageops'
import { filterPaletteItems, pushRecent } from '../paletteModel'
import { Row } from './Row'

interface Item {
  id: string
  group: 'cmd' | 'nav' | 'open'
  label: string
  hint?: string
  /// ADR 0056-2（ui-audit 票 03）：'danger' 项渲染分级 + 执行前过确认层。
  risk?: 'danger'
  /// 参与匹配的额外关键词（英文动词/缩写），不进显示（ui-audit 票 09）。
  keywords?: string[]
  run: () => unknown
}

export function CommandPalette({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation()
  const { team, artifacts, openTab, setRailOpen, setSplitOpen, setActiveTab, railOpen, splitOpen, tabs, activeTab, closeTab, invalidate, pushToast, pending } = useUiStore()
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
      // ui-audit 票 03（ADR 0056-2）：rewind/skip 标 danger——
      // 渲染分级 + 执行前过确认层（runStageOp 内部 askConfirm）。
      // ADR 0069：指针操作（退回/跳过/暂停/恢复/阶段盖章）不再进命令面板。
      // 最终验收仍在待决卡。全员休眠留下。
      // Fullstack QA 2026-10-05: owners searched “检查”/“测试” and saw
      // no command. Match ordinary Chinese verbs as well as the label.
      { id: 'c-checks', group: 'cmd', label: t('palette.checks'), keywords: ['check', 'verify', '检查', '测试', '检验'], run: () => api.runChecks() },
      { id: 'c-sleep', group: 'cmd', label: t('palette.sleepAll'), keywords: ['sleep'], run: () => runStageOp('sleepAll') },
      // ui-audit 票 09（P1-5 残余）：冰山 IPC 补入口——这两条此前只有
      // 纯 invoke 通道，面板不可达。
      {
        id: 'c-invariant', group: 'cmd', label: t('palette.invariantCheck'), keywords: ['inv', 'invariant', 'audit'],
        run: async () => {
          const n = await api.invariantCheck()
          pushToast(t('palette.invariantDone', { count: n }), n > 0 ? 'err' : 'ok')
        },
      },
      {
        // L3 不可逆：不直接发布——请求/聚焦发布卡，卡内按钮确认（ADR 0056）。
        id: 'c-publish', group: 'cmd', label: t('palette.requestPublish'), keywords: ['pub', 'publish', 'release'],
        run: async () => {
          if (!pending.some((x) => x.kind === 'publish')) await api.requestPublish('origin')
          await invalidate()
          // 票 05：待决区已撤，发布卡在弹窗里。invalidate 后的新 id 也会再弹；
          // 这里显式打开，覆盖「负责人刚收起过同一张卡」的情况。
          useUiStore.getState().openPendingDialog()
        },
      },
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
  }, [t, team, artifacts, openTab, setRailOpen, setSplitOpen, setActiveTab, railOpen, splitOpen, closeTab, activeTab, tabs, pending, pushToast, invalidate])

  // ui-audit 票 09（P3-19）：keywords 参与匹配（英文动词/缩写），
  // 最近使用项置顶（localStorage hexagon.palette.recents）。
  const [recents, setRecents] = useState<string[]>(() => {
    try { return JSON.parse(localStorage.getItem('hexagon.palette.recents') || '[]') } catch { return [] }
  })
  const filtered = useMemo(() => filterPaletteItems(items, q, recents), [items, q, recents])

  const runItem = async (i: Item) => {
    onClose()
    // 票 09：执行失败走 toast 出口而非静默
    try {
      await i.run()
      const next = pushRecent(recents, i.id)
      setRecents(next)
      localStorage.setItem('hexagon.palette.recents', JSON.stringify(next))
    } catch (e) {
      pushToast(errText(e), 'err')
    }
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
        className="panel panel-float"
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
        <div role="listbox" style={{ flex: 1, overflowY: 'auto', padding: '6px' }}>
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
                    // ui-audit 票 12（P2-13）：条目可键盘聚焦+Enter 激活，
                    // aria-selected 报选中态（Row 原语）
                    <Row
                      key={i.id}
                      role="option"
                      selected={idx === sel}
                      onClick={() => void runItem(i)}
                      onMouseEnter={() => setSel(idx)}
                      style={{
                        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
                        padding: '7px 10px', borderRadius: 6, cursor: 'pointer', fontSize: 13,
                        background: idx === sel ? 'var(--bg-2)' : 'transparent',
                      }}
                    >
                      <span style={i.risk === 'danger' ? { color: 'var(--err)' } : undefined}>{i.label}</span>
                      {i.hint && <span className="dim3 mono" style={{ fontSize: 11 }}>{i.hint}</span>}
                    </Row>
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
