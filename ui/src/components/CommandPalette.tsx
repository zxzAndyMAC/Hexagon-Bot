// ⌘K 命令面板（票 29；ui-redesign 票 07 换皮 → components D18「Command in Dialog」，composer#palette）。
// 指令/导航/快速打开统一入口。指令项与 composer 文本指令同语义（同 api 命令通道）：
// /stamp /skip /rewind /pause /resume /sleep 走的都是这些 api.*。
// 外壳：共享 Dialog（居中偏上、宽 560、overlay、背景裁决快捷键被拦）；搜索行 48 高（search + 输入 + Esc kbd）
// → 分组（组标题 11.5 ink-3）→ 底栏（inset 底：↑↓ 移动 ↵ 执行 + 危险指令说明）。
// 2026-10-14 负责人裁决引入 cmdk（shadcn Command）：键盘 ↑↓↵、选中态、option 语义交给 cmdk；
// 过滤仍走 filterPaletteItems（shouldFilter=false）——keywords 匹配与最近使用置顶（ui-audit 票 09）是本仓语义，cmdk 的模糊打分会打乱。
import { useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, type ActionId } from '../keymap'
import { runStageOp } from '../stageops'
import { filterPaletteItems, pushRecent } from '../paletteModel'
import { Icon, type IconName } from './Icon'
import { Avatar } from '@/components/ui/avatar'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { Kbd } from '@/components/ui/kbd'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'

interface Item {
  id: string
  group: 'cmd' | 'nav' | 'open'
  label: string
  /** 行右侧灰描述（D18：名 + 灰描述 + 右侧 kbd） */
  desc?: string
  hint?: string
  icon?: IconName
  /** 快速打开 Agent 行用头像代替图标 */
  agent?: { id: string; role: string }
  mono?: boolean
  /// ADR 0056-2（ui-audit 票 03）：'danger' 项渲染分级 + 执行前过确认层。
  risk?: 'danger'
  /// 参与匹配的额外关键词（英文动词/缩写），不进显示（ui-audit 票 09）。
  keywords?: string[]
  run: () => unknown
}

export function CommandPalette({ onClose, onSettings }: { onClose: () => void; onSettings?: () => void }) {
  const { t } = useTranslation()
  const { team, artifacts, openTab, toggleSidebar, toggleCanvas, toggleCanvasFull, showConversation, toggleFocusMode, activeTab, closeTab, invalidate, pushToast, pending, setFlowDialogOpen } = useUiStore()
  const [q, setQ] = useState('')

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
      { id: 'c-checks', group: 'cmd', label: t('palette.checks'), desc: t('palette.checksDesc'), icon: 'test', keywords: ['check', 'verify', '检查', '测试', '检验'], run: () => api.runChecks() },
      { id: 'c-invariant', group: 'cmd', label: t('palette.invariantCheck'), icon: 'circle-check', keywords: ['inv', 'invariant', 'audit'],
        // ui-audit 票 09（P1-5 残余）：冰山 IPC 补入口——此前只有纯 invoke 通道，面板不可达。
        run: async () => {
          const n = await api.invariantCheck()
          pushToast(t('palette.invariantDone', { count: n }), n > 0 ? 'err' : 'ok')
        },
      },
      { id: 'c-sleep', group: 'cmd', label: t('palette.sleepAll'), icon: 'sleep', keywords: ['sleep'], run: () => runStageOp('sleepAll') },
      {
        // L3 不可逆：不直接发布——请求/聚焦发布卡，卡内按钮确认（ADR 0056）。
        id: 'c-publish', group: 'cmd', label: t('palette.requestPublish'), icon: 'publish', keywords: ['pub', 'publish', 'release'],
        run: async () => {
          if (!pending.some((x) => x.kind === 'publish')) await api.requestPublish('origin')
          await invalidate()
          // 票 05：待决区已撤，发布卡在弹窗里。invalidate 后的新 id 也会再弹；
          // 这里显式打开，覆盖「负责人刚收起过同一张卡」的情况。
          useUiStore.getState().openPendingDialog()
        },
      },
      // 导航：ADR 0088（票 02）⌘B 开关左侧栏；票 03（R-04 / R-06）分屏撤掉 → 画布全宽 ↔ 并排、专注模式；
      // 票 07（D18）补「开关画布」「查看流程与质量检查」「打开设置」，同步新动作。
      { id: 'n-sidebar', group: 'nav', label: t('palette.sidebar'), icon: 'panel-left', hint: bind('toggleSidebar'), run: () => toggleSidebar() },
      { id: 'n-canvas', group: 'nav', label: t('palette.canvas'), icon: 'panel-right', hint: bind('toggleCanvas'), run: () => toggleCanvas() },
      { id: 'n-canvas-full', group: 'nav', label: t('palette.canvasFull'), icon: 'expand', hint: bind('canvasFullWidth'), run: () => toggleCanvasFull() },
      { id: 'n-focus', group: 'nav', label: t('palette.focusMode'), icon: 'focus', hint: bind('focusMode'), run: () => toggleFocusMode() },
      { id: 'n-conversation', group: 'nav', label: t('palette.conversation'), icon: 'message', hint: bind('focusComposer'), run: () => showConversation() },
      { id: 'n-flow', group: 'nav', label: t('palette.flow'), icon: 'flow', keywords: ['flow', 'quality', '流程', '质量'], run: () => setFlowDialogOpen(true) },
      ...(onSettings ? [{ id: 'n-settings', group: 'nav' as const, label: t('palette.settings'), icon: 'settings' as const, hint: bind('settings'), keywords: ['settings', 'preferences'], run: onSettings }] : []),
      ...(activeTab ? [{
        id: 'n-close', group: 'nav' as const, label: t('palette.closeTab'), icon: 'close' as const, hint: bind('closeTab'),
        run: () => closeTab(activeTab),
      }] : []),
      // 快速打开：产物（file-text + 等宽路径 +「产物」）、Agent（头像 + 角色名 +「Agent」）
      ...artifacts.map((a) => ({
        id: `o-f-${a.path}-v${a.version}`, group: 'open' as const, mono: true, icon: 'file-text' as const,
        label: `${a.path} v${a.version}`, desc: t('palette.artifactDesc'), keywords: [t('palette.openArtifact')],
        run: () => openTab({ id: `art:${a.path}`, kind: 'artifact', title: a.path, path: a.path }),
      })),
      ...team.map((m) => ({
        id: `o-a-${m.id}`, group: 'open' as const, agent: { id: m.id, role: m.role },
        label: m.role, desc: t('palette.agentDesc'), keywords: [t('palette.openAgent'), m.id],
        run: () => openTab({ id: `agent:${m.id}`, kind: 'agent', title: m.role, agentId: m.id, role: m.role }),
      })),
    ]
    return list
  }, [t, team, artifacts, openTab, toggleSidebar, toggleCanvas, toggleCanvasFull, showConversation, toggleFocusMode, closeTab, activeTab, pending, pushToast, invalidate, setFlowDialogOpen, onSettings])

  // ui-audit 票 09（P3-19）：keywords 参与匹配（英文动词/缩写），
  // 最近使用项置顶（localStorage hexagon.palette.recents）。
  const [recents, setRecents] = useState<string[]>(() => {
    try { return JSON.parse(localStorage.getItem('hexagon.palette.recents') || '[]') } catch { return [] }
  })
  const filtered = useMemo(() => filterPaletteItems(items, q, recents), [items, q, recents])
  const groups: { key: Item['group']; title: string }[] = [
    { key: 'cmd', title: t('palette.commands') },
    { key: 'nav', title: t('palette.nav') },
    { key: 'open', title: t('palette.open') },
  ]
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

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent aria-describedby={undefined} data-command-palette
        className="top-[90px] w-[min(560px,calc(100vw-48px))] translate-y-0 gap-0 overflow-hidden p-0">
        <DialogTitle className="sr-only">{t('palette.title')}</DialogTitle>
        <Command shouldFilter={false} loop label={t('palette.title')}>
          <CommandInput autoFocus value={q} onValueChange={setQ} placeholder={t('palette.placeholder')} trailing={<Kbd>Esc</Kbd>} />
          <CommandList aria-label={t('palette.title')}>
            {/* A19 空态小号（feedback#empty） */}
            <CommandEmpty>
              <span className="flex flex-col items-center gap-1.5">
                <Icon name="search" size={20} />
                <span>{q.trim() ? t('palette.emptyFor', { q: q.trim() }) : t('palette.empty')}</span>
              </span>
            </CommandEmpty>
            {/* 显示顺序 = 分组顺序；组内保持 filterPaletteItems 的排序（最近使用在前） */}
            {groups.map((g) => {
              const rows = filtered.filter((i) => i.group === g.key)
              if (!rows.length) return null
              return (
                <CommandGroup key={g.key} heading={g.title}>
                  {rows.map((i) => <PaletteRow key={i.id} item={i} onRun={() => void runItem(i)} />)}
                </CommandGroup>
              )
            })}
          </CommandList>
        </Command>
        <div className="flex h-9 items-center gap-1.5 border-t border-line bg-inset px-3.5 text-[12px] text-ink-3">
          <Kbd>↑↓</Kbd>{t('palette.move')}<Kbd className="ml-1.5">↵</Kbd>{t('palette.run')}
          <span className="flex-1" />{t('palette.dangerNote')}
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** D18 行 = 图标 + 名 13/500 + 灰描述 + 右侧 kbd；高亮 = --hover 底（cmdk data-selected，同 A16 mi.on）。 */
function PaletteRow({ item, onRun }: { item: Item; onRun: () => void }) {
  const avatars = useUiStore((s) => s.avatars)
  let lead: ReactNode = null
  if (item.agent) lead = <Avatar agentId={item.agent.id} name={item.agent.role} size={18} src={avatars[item.agent.id]} />
  else if (item.icon) lead = <Icon name={item.icon} className="text-ink-2" size={14} />
  return (
    <CommandItem value={item.id} onSelect={onRun}>
      <span className="grid w-[18px] shrink-0 place-items-center">{lead}</span>
      <span className={`min-w-0 truncate ${item.mono ? 'font-mono font-normal text-[12.5px]' : ''} ${item.risk === 'danger' ? 'text-red-ink' : ''}`}>{item.label}</span>
      <span className="ml-auto flex shrink-0 items-center gap-2 text-[12px] font-normal text-ink-3">
        {item.desc && <span>{item.desc}</span>}
        {item.hint && <Kbd>{item.hint}</Kbd>}
      </span>
    </CommandItem>
  )
}
