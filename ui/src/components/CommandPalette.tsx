// ⌘K 命令面板（票 29；ui-redesign 票 07 换皮 → components D18「Command in Dialog」，composer#palette）。
// 指令/导航/快速打开统一入口。指令项与 composer 文本指令同语义（同 api 命令通道）：
// /stamp /skip /rewind /pause /resume /sleep 走的都是这些 api.*。
// 外壳：共享 Dialog（居中偏上、宽 560、overlay、背景裁决快捷键被拦）；搜索行 48 高（search + 输入 + Esc kbd）
// → 分组（组标题 11.5 ink-3）→ 底栏（inset 底：↑↓ 移动 ↵ 执行 + 危险指令说明）。
// 2026-10-14 负责人裁决引入 cmdk（shadcn Command）：键盘 ↑↓↵、选中态、option 语义交给 cmdk；
// 过滤仍走 filterPaletteItems（shouldFilter=false）——keywords 匹配与最近使用置顶（ui-audit 票 09）是本仓语义，cmdk 的模糊打分会打乱。
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { api, errText, type PushTargets, type ReleaseTarget } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, type ActionId } from '../keymap'
import { runStageOp } from '../stageops'
import { filterPaletteItems, pushRecent } from '../paletteModel'
import { laterBlockedReason } from '../sessionPill'
import { adjacentTab, closeOtherTabs, closeTabsToRight, copyTabPath, otherTabs, stepTab, tabPath, tabsRightOf } from '../tabActions'
import { changeMemberAvatar, copyMemberName, memberRowTarget, mentionMember, openMemberDetail, restoreMemberAvatar } from '../memberActions'
import { dispatchMessageAction, messageOnRail, messageTarget } from '../messageActions'
import { insertPathToComposer, revealLabelKey, revealPath, treeRowTarget } from '../fileActions'
import { openPathWith, useOpenWithCandidates, type OpenWithApp, type OpenWithState } from '../openWith'
import { AppIcon } from './OpenWithMenu'
import { treeInsertPath } from '../composerAtoms'
import { Icon, type IconName } from './Icon'
import { Avatar } from '@/components/ui/avatar'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { Kbd } from '@/components/ui/kbd'
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command'

/** 面板视图：全量 / 「打开方式」（#31：openWith 键位或「打开方式…」项进入，只列编辑器）。 */
export type PaletteScope = 'all' | 'openWith'

interface Item {
  id: string
  group: 'cmd' | 'nav' | 'open' | 'openWith'
  label: string
  /** 行右侧灰描述（D18：名 + 灰描述 + 右侧 kbd） */
  desc?: string
  hint?: string
  icon?: IconName
  /** 快速打开 Agent 行用头像代替图标 */
  agent?: { id: string; role: string }
  /** #31「打开方式」行用系统应用图标代替图标 */
  app?: OpenWithApp
  mono?: boolean
  /// ADR 0056-2（ui-audit 票 03）：'danger' 项渲染分级 + 执行前过确认层。
  risk?: 'danger'
  /// 参与匹配的额外关键词（英文动词/缩写），不进显示（ui-audit 票 09）。
  keywords?: string[]
  /// owner-followups-1015 #15：此刻做不了——灰显、不可选，desc 写原因（「发布…」没有发布目标）。
  disabled?: boolean
  /// 执行后不关面板（「发布…」有多个目标时先进选择步；#31「打开方式…」切到编辑器视图）。
  keepOpen?: boolean
  run: () => unknown
}

/** 发起发布：只入队发布确认卡（必问、永不记忆），再把卡弹出来——同推送，不在面板里直接执行。 */
async function requestRelease(targetId: string, invalidate: () => Promise<void>) {
  await api.requestRelease(targetId)
  await invalidate()
  useUiStore.getState().openPendingDialog()
}

export function CommandPalette({ onClose, onSettings, scope: initialScope = 'all' }: { onClose: () => void; onSettings?: () => void; scope?: PaletteScope }) {
  const { t } = useTranslation()
  const { team, artifacts, openTab, toggleSidebar, toggleCanvas, toggleCanvasFull, showConversation, toggleFocusMode, activeTab, tabs, closeTab, invalidate, pushToast, pending, setFlowDialogOpen, setSessionLaterOpen, sessionSummary } = useUiStore()
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
  // owner-followups-1015 #15（ADR 0093 后续裁决「⌘K 拆推送 / 发布…」）：打开面板时读一次发布目标。
  // null = 还在读；读失败按 0 个处理（灰显写因，不出死项）。picking = 多个目标时的选择步。
  const [releases, setReleases] = useState<ReleaseTarget[] | null>(null)
  const [picking, setPicking] = useState(false)
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

  const items = useMemo<Item[]>(() => {
    const bind = (a: ActionId) => formatBinding(bindingFor(a))
    if (scope === 'openWith') return openWithItems(t, openTarget, targetApps, projectApps)
    // 「发布…」：0 个目标灰显写因、1 个直接发起、多个先选（选择步列出每个目标，名 + 命令）。
    const releaseItem = (): Item => {
      const base = { id: 'c-release', group: 'cmd' as const, label: t('palette.requestRelease'), icon: 'publish' as const,
        keywords: ['release', 'publish', 'deploy', '发布', '发版'] }
      if (releases === null) return { ...base, disabled: true, desc: t('palette.releaseLoading'), run: () => {} }
      if (releases.length === 0) return { ...base, disabled: true, desc: t('palette.releaseNone'), run: () => {} }
      if (releases.length === 1) return { ...base, desc: releases[0].name, run: () => requestRelease(releases[0].id, invalidate) }
      return { ...base, desc: t('palette.releaseCount', { count: releases.length }), keepOpen: true,
        run: () => { setPicking(true); setQ('') } }
    }
    if (picking && releases) {
      return releases.map((r) => ({
        id: `r-${r.id}`, group: 'cmd' as const, label: r.name, desc: r.command, icon: 'publish' as const,
        keywords: [r.command], run: () => requestRelease(r.id, invalidate),
      }))
    }
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
      releaseItem(),
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
      ...(activeTab ? [{
        id: 'n-close', group: 'nav' as const, label: t('palette.closeTab'), icon: 'close' as const, hint: bind('closeTab'),
        run: () => closeTab(activeTab),
      }] : []),
      // owner-followups-1015 #05（A16b「定位」）：右键菜单的动作都要在 ⌘K 找得到；作用于当前页签，
      // 此刻做不了（没有其他 / 右侧页签）或种类不适用（非文件 / 产物页签无路径）就不列。
      ...(activeTab && otherTabs(tabs, activeTab).length ? [{
        id: 'n-close-others', group: 'nav' as const, label: t('palette.closeOtherTabs'), icon: 'close' as const, hint: bind('closeOtherTabs'),
        run: () => closeOtherTabs(activeTab),
      }] : []),
      ...(activeTab && tabsRightOf(tabs, activeTab).length ? [{
        id: 'n-close-right', group: 'nav' as const, label: t('palette.closeTabsToRight'), icon: 'close' as const, hint: bind('closeTabsToRight'),
        run: () => closeTabsToRight(activeTab),
      }] : []),
      // owner-followups-1015 #06（A22「溢出」）：上 / 下一个页签，只有一个页签时不列。
      ...(activeTab && adjacentTab(tabs, activeTab, 1) ? [
        { id: 'n-next-tab', group: 'nav' as const, label: t('palette.nextTab'), icon: 'chevron-right' as const, hint: bind('nextTab'), run: () => stepTab(1) },
        { id: 'n-prev-tab', group: 'nav' as const, label: t('palette.prevTab'), icon: 'chevron-left' as const, hint: bind('prevTab'), run: () => stepTab(-1) },
      ] : []),
      ...(activeTab && tabPath(tabs.find((x) => x.id === activeTab)) ? [{
        id: 'n-copy-path', group: 'nav' as const, label: t('palette.copyPath'), icon: 'copy' as const, hint: bind('copyPath'),
        run: () => copyTabPath(activeTab),
      }] : []),
      ...(memberTarget ? [
        // #29：「打开详情」补进面板，与同菜单其他动作一致。
        { id: 'c-open-member-detail', group: 'cmd' as const, label: t('palette.openMemberDetail', { role: memberTarget.role }), icon: 'agent' as const,
          hint: bind('openMemberDetail'), run: () => openMemberDetail(memberTarget.id) },
        { id: 'c-mention-member', group: 'cmd' as const, label: t('palette.mentionMember', { role: memberTarget.role }), icon: 'message' as const,
          hint: bind('mentionMember'), run: () => mentionMember(memberTarget.id) },
        { id: 'c-copy-member-name', group: 'cmd' as const, label: t('palette.copyMemberName', { role: memberTarget.role }), icon: 'copy' as const,
          hint: bind('copyMemberName'), run: () => copyMemberName(memberTarget.id) },
        // #18：「恢复默认」照列——面板是同步列表，角色有无自定义头像在执行时判（没有则提示，不发删除）。
        { id: 'c-change-member-avatar', group: 'cmd' as const, label: t('palette.changeMemberAvatar', { role: memberTarget.role }), icon: 'image' as const,
          hint: bind('changeMemberAvatar'), run: () => changeMemberAvatar(memberTarget.id) },
        { id: 'c-restore-member-avatar', group: 'cmd' as const, label: t('palette.restoreMemberAvatar', { role: memberTarget.role }), icon: 'undo' as const,
          hint: bind('restoreMemberAvatar'), run: () => void restoreMemberAvatar(memberTarget.id) },
      ] : []),
      ...(msgHost ? [
        { id: 'c-copy-message', group: 'cmd' as const, label: t('palette.copyMessage'), icon: 'copy' as const,
          hint: bind('copyMessage'), run: () => dispatchMessageAction(msgHost, 'copy') },
        { id: 'c-quote-message', group: 'cmd' as const, label: t('palette.quoteMessage'), icon: 'message' as const,
          hint: bind('quoteMessage'), run: () => dispatchMessageAction(msgHost, 'quote') },
        ...(messageOnRail(msgHost) ? [{ id: 'c-locate-message', group: 'nav' as const, label: t('palette.locateInRail'), icon: 'crosshair' as const,
          hint: bind('locateInRail'), run: () => dispatchMessageAction(msgHost, 'locate') }] : []),
      ] : []),
      // owner-followups-1015 #07（A16b 文件树行）：插入到输入条 / 在访达中显示，同样作用于当前文件 / 产物页签。
      ...(activeTab && tabPath(tabs.find((x) => x.id === activeTab)) ? [{
        id: 'n-insert-path', group: 'nav' as const, label: t('palette.insertPath'), icon: 'message' as const, hint: bind('insertPath'),
        run: () => { const p = tabPath(tabs.find((x) => x.id === activeTab)); const ins = p && treeInsertPath({ path: p, kind: 'file' }); if (ins) insertPathToComposer(ins) },
      }, {
        id: 'n-reveal-path', group: 'nav' as const, label: t(revealLabelKey()), icon: 'folder' as const, hint: bind('revealPath'),
        run: () => { const p = tabPath(tabs.find((x) => x.id === activeTab)); if (p) void revealPath(p) },
      }] : []),
      // #31：「打开方式…」切到只列编辑器的视图（同 openWith 键位），不关面板。
      { id: 'n-open-with', group: 'nav', label: t('palette.openWith'), icon: 'external', hint: bind('openWith'), keepOpen: true,
        keywords: ['open with', 'ide', 'editor', 'vscode', 'xcode', '编辑器'], run: () => { setScope('openWith'); setQ('') } },
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

/** #31「打开方式」视图：目标（文件 / 目录）的编辑器在前，整个项目的在后；名称不翻译。 */
function openWithItems(t: TFunction, target: string | null, targetApps: OpenWithState, projectApps: OpenWithState): Item[] {
  const name = target ? target.split('/').filter(Boolean).pop() ?? target : ''
  const rows: Item[] = []
  if (target && targetApps.kind === 'ready') {
    for (const app of targetApps.apps) {
      rows.push({ id: `w-${app.id}-${target}`, group: 'openWith', app, label: t('palette.openWithApp', { app: app.name }),
        // #32：系统默认应用在灰描述后标「（默认）」，同子菜单
        desc: app.is_default ? `${name} ${t('ctx.openWithDefault')}` : name,
        keywords: [app.id, target], run: () => openPathWith(target, app.id) })
    }
  }
  if (projectApps.kind === 'ready') {
    for (const app of projectApps.apps) {
      rows.push({ id: `wp-${app.id}`, group: 'openWith', app, label: t('palette.openProjectWith', { app: app.name }), desc: t('palette.wholeProject'),
        keywords: [app.id], run: () => openPathWith('', app.id) })
    }
  }
  return rows
}

/** 「打开方式」视图没有行时说清为什么：还在读 / 读失败 / 本机没有可用编辑器。 */
function openWithEmptyKey(a: OpenWithState, b: OpenWithState): string {
  if (a.kind === 'error' || b.kind === 'error') return 'ctx.openWithAppsError'
  if (b.kind === 'loading') return 'ctx.openWithAppsLoading'
  return 'ctx.openWithNoApps'
}

/** #13 推送项：有上游（或还没读到目标）= 一项「推送」，描述写目标远程；
 *  没有上游 = 每个远程一项「推送到 X」，由负责人选；一个远程都没有 = 一项，执行时说明原因。 */
function pushItems(
  t: TFunction, targets: PushTargets | null, startPush: (remote: string) => Promise<void>,
  pushToast: (msg: string, tone?: 'ok' | 'err') => void,
): Item[] {
  const keywords = ['push', 'git', 'pub', 'publish']
  if (targets && !targets.upstream && targets.remotes.length > 0) {
    return targets.remotes.map((remote) => ({
      id: `c-push-${remote}`, group: 'cmd' as const, label: t('palette.pushTo', { remote }), desc: t('palette.pushNoUpstream'),
      icon: 'publish' as const, keywords: [...keywords, remote], run: () => startPush(remote),
    }))
  }
  return [{
    id: 'c-push', group: 'cmd', label: t('palette.requestPush'), icon: 'publish', keywords,
    desc: targets?.upstream ? targets.upstream : undefined,
    run: async () => {
      const current = targets ?? await api.pushTargets()
      if (current.upstream) return startPush(current.upstream)
      pushToast(t(current.remotes.length ? 'palette.pushChooseRemote' : 'palette.pushNoRemote'), 'err')
    },
  }]
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
