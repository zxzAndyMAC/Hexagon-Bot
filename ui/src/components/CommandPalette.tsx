// ⌘K 命令面板（票 29；ui-redesign 票 07 换皮 → components D18「Command in Dialog」，composer#palette）。
// 指令/导航/快速打开统一入口。指令项与 composer 文本指令同语义（同 api 命令通道）：
// /stamp /skip /rewind /pause /resume /sleep 走的都是这些 api.*。
// 外壳：共享 Dialog（居中偏上、宽 560、overlay、背景裁决快捷键被拦）；搜索行 48 高（search + 输入 + Esc kbd）
// → 分组（组标题 11.5 ink-3）→ 底栏（inset 底：↑↓ 移动 ↵ 执行 + 危险指令说明）。
// 2026-10-14 负责人裁决引入 cmdk（shadcn Command）：键盘 ↑↓↵、选中态、option 语义交给 cmdk；
// 过滤仍走 filterPaletteItems（shouldFilter=false）——keywords 匹配与最近使用置顶（ui-audit 票 09）是本仓语义，cmdk 的模糊打分会打乱。
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type PushTargets, type ReleaseTarget } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, type ActionId } from '../keymap'
import { runStageOp } from '../stageops'
import { filterPaletteItems, pushRecent } from '../paletteModel'
import { laterBlockedReason } from '../sessionPill'
import { tabPath } from '../tabActions'
import { memberRowTarget } from '../memberActions'
import { messageTarget } from '../messageActions'
import { treeRowTarget } from '../fileActions'
import { useOpenWithCandidates } from '../openWith'
import { AppIcon } from './OpenWithMenu'
import { memberItems, messageItems, openWithEmptyKey, openWithItems, pathItems, pushItems, quickOpenItems, releaseItem, releasePickItems, tabItems, type Item } from './paletteItems'
import { Icon } from './Icon'
import { Avatar } from '@/components/ui/avatar'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { Kbd } from '@/components/ui/kbd'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'

/** 面板视图：全量 / 「打开方式」（#31：openWith 键位或「打开方式…」项进入，只列编辑器）。 */
export type PaletteScope = 'all' | 'openWith'

/** 打开面板时读一次推送 / 发布目标，并给出「推送到某远程」的发起函数。 */
function usePushReleaseTargets() {
  const { t } = useTranslation()
  const { invalidate, pushToast, pending } = useUiStore()
  // owner-followups-1015 #15（ADR 0093 后续裁决「⌘K 拆推送 / 发布…」）：打开面板时读一次发布目标。
  // null = 还在读；读失败按 0 个处理（灰显写因，不出死项）。
  const [releases, setReleases] = useState<ReleaseTarget[] | null>(null)
  useEffect(() => {
    let live = true
    api.releaseTargets().then((rows) => { if (live) setReleases(rows) }, () => { if (live) setReleases([]) })
    return () => { live = false }
  }, [])
  // owner-followups-1015 #13（ADR 0093 后续裁决）：推送目标 = 基线分支的上游远程，不再写死 origin；
  // 没有上游时列出每个远程让负责人选。读失败不挡面板：「推送」项执行时再读一次，失败走 toast。
  const [pushTargets, setPushTargets] = useState<PushTargets | null>(null)
  useEffect(() => {
    let active = true
    api.pushTargets().then((v) => { if (active) setPushTargets(v) }, () => { /* 执行时重读，错误在那里出 toast */ })
    return () => { active = false }
  }, [])
  const startPush = useCallback(async (remote: string) => {
    // L3 不可逆：不直接推送——请求/聚焦推送卡（原发布卡，ADR 0093 改名），卡内按钮确认（ADR 0056）；
    // 唯一例外是负责人在推送卡上「允许本项目」过同一远程 + 分支（#13），宿主当场推送并回执。
    if (!pending.some((x) => x.kind === 'push')) {
      const req = await api.requestPush(remote)
      if (req.pushed) { pushToast(t('palette.pushed', { remote: req.pushed.remote, branch: req.pushed.baseline }), 'ok'); return }
    }
    await invalidate()
    // 票 05：待决区已撤，推送卡在弹窗里。invalidate 后的新 id 也会再弹；
    // 这里显式打开，覆盖「负责人刚收起过同一张卡」的情况。
    useUiStore.getState().openPendingDialog()
  }, [pending, invalidate, pushToast, t])
  return { releases, pushTargets, startPush }
}

export function CommandPalette({ onClose, onSettings, scope: initialScope = 'all' }: { onClose: () => void; onSettings?: () => void; scope?: PaletteScope }) {
  const { t } = useTranslation()
  const { team, artifacts, openTab, toggleSidebar, toggleCanvas, toggleCanvasFull, showConversation, toggleFocusMode, activeTab, tabs, closeTab, invalidate, pushToast, setFlowDialogOpen, setSessionLaterOpen, sessionSummary } = useUiStore()
  const laterBlocked = laterBlockedReason(sessionSummary)
  const [q, setQ] = useState('')
  // owner-followups-1015 #08（A16b「定位」）：成员行 / 消息动作作用于打开面板前焦点所在的对象——
  // 首次渲染时焦点还没进面板，这里记下来；目标不在成员行 / 消息上就不列这些项。
  const [origin] = useState(() => document.activeElement)
  const memberTarget = useMemo(() => team.find((m) => m.id === memberRowTarget(origin)), [team, origin])
  const msgHost = useMemo(() => messageTarget(origin), [origin])
  // owner-followups-1015 #31（A16b「打开方式」）：作用对象 = 打开面板前的焦点文件树行（链接除外），
  // 否则当前文件 / 产物页签；另列整个项目（路径空串）。只在「打开方式」视图读候选。
  const [scope, setScope] = useState<PaletteScope>(initialScope)
  const [openTarget] = useState(() => {
    const row = treeRowTarget()
    if (row) return row.kind === 'link' ? null : row.path
    return tabPath(tabs.find((x) => x.id === activeTab))
  })
  const targetApps = useOpenWithCandidates(scope === 'openWith' ? openTarget : null).state
  const projectApps = useOpenWithCandidates(scope === 'openWith' ? '' : null).state
  const [picking, setPicking] = useState(false)
  const { releases, pushTargets, startPush } = usePushReleaseTargets()

  const items = useMemo<Item[]>(() => {
    const bind = (a: ActionId) => formatBinding(bindingFor(a))
    if (scope === 'openWith') return openWithItems(t, openTarget, targetApps, projectApps)
    if (picking && releases) return releasePickItems(releases, invalidate)
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
      ...pushItems(t, pushTargets, startPush, pushToast),
      releaseItem(t, releases, invalidate, () => { setPicking(true); setQ('') }),
      // owner-followups-1015 #11（C12a）：「稍后继续…」——会话卡收成胶囊后健康时看不见，入口进 ⌘K 与头部 ⋯ 菜单。
      // 面板关闭后由会话胶囊的状态槽弹出时间弹层；做不了（无会话 / 已结束 / 已有预约）灰显写原因。
      { id: 'c-session-later', group: 'cmd', label: t('session.scheduleLater'), icon: 'clock', hint: bind('sessionScheduleOnce'),
        keywords: ['later', 'schedule', 'resume', '稍后', '预约', '继续'],
        ...(laterBlocked ? { disabled: true, desc: t(laterBlocked) } : {}),
        run: () => setSessionLaterOpen(true) },
      // 导航：ADR 0088（票 02）⌘B 开关左侧栏；票 03（R-04 / R-06）分屏撤掉 → 画布全宽 ↔ 并排、专注模式；
      // 票 07（D18）补「开关画布」「查看流程与质量检查」「打开设置」，同步新动作。
      { id: 'n-sidebar', group: 'nav', label: t('palette.sidebar'), icon: 'panel-left', hint: bind('toggleSidebar'), run: () => toggleSidebar() },
      { id: 'n-canvas', group: 'nav', label: t('palette.canvas'), icon: 'panel-right', hint: bind('toggleCanvas'), run: () => toggleCanvas() },
      { id: 'n-canvas-full', group: 'nav', label: t('palette.canvasFull'), icon: 'expand', hint: bind('canvasFullWidth'), run: () => toggleCanvasFull() },
      { id: 'n-focus', group: 'nav', label: t('palette.focusMode'), icon: 'focus', hint: bind('focusMode'), run: () => toggleFocusMode() },
      { id: 'n-conversation', group: 'nav', label: t('palette.conversation'), icon: 'message', hint: bind('focusComposer'), run: () => showConversation() },
      { id: 'n-flow', group: 'nav', label: t('palette.flow'), icon: 'flow', keywords: ['flow', 'quality', '流程', '质量'], run: () => setFlowDialogOpen(true) },
      ...(onSettings ? [{ id: 'n-settings', group: 'nav' as const, label: t('palette.settings'), icon: 'settings' as const, hint: bind('settings'), keywords: ['settings', 'preferences'], run: onSettings }] : []),
      ...tabItems(t, bind, tabs, activeTab, closeTab),
      ...memberItems(t, bind, memberTarget),
      ...messageItems(t, bind, msgHost),
      ...pathItems(t, bind, tabs, activeTab),
      // #31：「打开方式…」切到只列编辑器的视图（同 openWith 键位），不关面板。
      { id: 'n-open-with', group: 'nav', label: t('palette.openWith'), icon: 'external', hint: bind('openWith'), keepOpen: true,
        keywords: ['open with', 'ide', 'editor', 'vscode', 'xcode', '编辑器'], run: () => { setScope('openWith'); setQ('') } },
      ...quickOpenItems(t, artifacts, team, openTab),
    ]
    return list
  }, [t, scope, openTarget, targetApps, projectApps, memberTarget, msgHost, team, artifacts, openTab, toggleSidebar, toggleCanvas, toggleCanvasFull, showConversation, toggleFocusMode, closeTab, activeTab, tabs, pushToast, setFlowDialogOpen, setSessionLaterOpen, laterBlocked, onSettings, pushTargets, startPush, invalidate, releases, picking])

  // ui-audit 票 09（P3-19）：keywords 参与匹配（英文动词/缩写），
  // 最近使用项置顶（localStorage hexagon.palette.recents）。
  const [recents, setRecents] = useState<string[]>(() => {
    try { return JSON.parse(localStorage.getItem('hexagon.palette.recents') || '[]') } catch { return [] }
  })
  const filtered = useMemo(() => filterPaletteItems(items, q, recents), [items, q, recents])
  const groups: { key: Item['group']; title: string }[] = [
    { key: 'cmd', title: picking ? t('palette.releaseChoose') : t('palette.commands') },
    { key: 'nav', title: t('palette.nav') },
    { key: 'open', title: t('palette.open') },
    { key: 'openWith', title: t('ctx.openWith') },
  ]
  const runItem = async (i: Item) => {
    if (i.disabled) return
    if (i.keepOpen) { i.run(); return }
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
          {/* 选择步（发布目标）：空输入框上按退格回到全部指令 */}
          <CommandInput autoFocus value={q} onValueChange={setQ} placeholder={picking ? t('palette.releaseChoose') : t('palette.placeholder')} trailing={<Kbd>Esc</Kbd>}
            onKeyDown={(e) => { if (picking && e.key === 'Backspace' && !q) { e.preventDefault(); setPicking(false) } }} />
          <CommandList aria-label={t('palette.title')}>
            {/* A19 空态小号（feedback#empty） */}
            <CommandEmpty>
              <span className="flex flex-col items-center gap-1.5">
                <Icon name="search" size={20} />
                <span>{q.trim() ? t('palette.emptyFor', { q: q.trim() }) : scope === 'openWith' ? t(openWithEmptyKey(targetApps, projectApps)) : t('palette.empty')}</span>
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
  if (item.app) lead = <AppIcon app={item.app} />
  else if (item.agent) lead = <Avatar agentId={item.agent.id} name={item.agent.role} size={18} src={avatars[item.agent.id]} />
  else if (item.icon) lead = <Icon name={item.icon} className="text-ink-2" size={14} />
  return (
    <CommandItem value={item.id} onSelect={onRun} disabled={item.disabled} data-palette-item={item.id}>
      <span className="grid w-[18px] shrink-0 place-items-center">{lead}</span>
      <span className={`min-w-0 truncate ${item.mono ? 'font-mono font-normal text-[12.5px]' : ''} ${item.risk === 'danger' ? 'text-red-ink' : ''}`}>{item.label}</span>
      <span className="ml-auto flex shrink-0 items-center gap-2 text-[12px] font-normal text-ink-3">
        {item.desc && <span>{item.desc}</span>}
        {item.hint && <Kbd>{item.hint}</Kbd>}
      </span>
    </CommandItem>
  )
}
