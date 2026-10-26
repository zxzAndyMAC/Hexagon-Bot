// ⌘K 命令面板的条目构造（票 34 自 CommandPalette.tsx 拆出，行为不变）：按作用对象分组的纯函数，
// 组件只负责读状态、拼顺序、渲染。条目顺序 = 组内显示顺序（filterPaletteItems 只把最近使用置顶）。
import type { TFunction } from 'i18next'
import { api, type PushTargets, type ReleaseTarget } from '../api'
import { useUiStore, type WorkTab } from '../store'
import { adjacentTab, closeOtherTabs, closeTabsToRight, copyTabPath, otherTabs, stepTab, tabPath, tabsRightOf } from '../tabActions'
import { changeMemberAvatar, copyMemberName, mentionMember, openMemberDetail, restoreMemberAvatar } from '../memberActions'
import { dispatchMessageAction, messageOnRail } from '../messageActions'
import { insertPathToComposer, revealLabelKey, revealPath } from '../fileActions'
import { openPathWith, type OpenWithApp, type OpenWithState } from '../openWith'
import { treeInsertPath } from '../composerAtoms'
import type { ActionId } from '../keymap'
import type { IconName } from './Icon'

/** 键位提示：动作 → 当前绑定的显示形。 */
export type Bind = (a: ActionId) => string

export interface Item {
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
export async function requestRelease(targetId: string, invalidate: () => Promise<void>) {
  await api.requestRelease(targetId)
  await invalidate()
  useUiStore.getState().openPendingDialog()
}

/** 「发布…」：0 个目标灰显写因、1 个直接发起、多个先选（选择步列出每个目标，名 + 命令）。 */
export function releaseItem(t: TFunction, releases: ReleaseTarget[] | null, invalidate: () => Promise<void>, startPicking: () => void): Item {
  const base = { id: 'c-release', group: 'cmd' as const, label: t('palette.requestRelease'), icon: 'publish' as const,
    keywords: ['release', 'publish', 'deploy', '发布', '发版'] }
  if (releases === null) return { ...base, disabled: true, desc: t('palette.releaseLoading'), run: () => {} }
  if (releases.length === 0) return { ...base, disabled: true, desc: t('palette.releaseNone'), run: () => {} }
  if (releases.length === 1) return { ...base, desc: releases[0].name, run: () => requestRelease(releases[0].id, invalidate) }
  return { ...base, desc: t('palette.releaseCount', { count: releases.length }), keepOpen: true, run: startPicking }
}

/** 「发布…」的选择步：每个目标一行（名 + 命令）。 */
export function releasePickItems(releases: ReleaseTarget[], invalidate: () => Promise<void>): Item[] {
  return releases.map((r) => ({
    id: `r-${r.id}`, group: 'cmd' as const, label: r.name, desc: r.command, icon: 'publish' as const,
    keywords: [r.command], run: () => requestRelease(r.id, invalidate),
  }))
}

/** 作用于当前页签的导航项（关闭 / 关闭其他 / 关闭右侧 / 上下页签 / 复制路径）。 */
export function tabItems(t: TFunction, bind: Bind, tabs: WorkTab[], activeTab: string | null, closeTab: (id: string) => void): Item[] {
  if (!activeTab) return []
  return [
    { id: 'n-close', group: 'nav', label: t('palette.closeTab'), icon: 'close', hint: bind('closeTab'), run: () => closeTab(activeTab) },
    // owner-followups-1015 #05（A16b「定位」）：右键菜单的动作都要在 ⌘K 找得到；作用于当前页签，
    // 此刻做不了（没有其他 / 右侧页签）或种类不适用（非文件 / 产物页签无路径）就不列。
    ...(otherTabs(tabs, activeTab).length ? [{
      id: 'n-close-others', group: 'nav' as const, label: t('palette.closeOtherTabs'), icon: 'close' as const, hint: bind('closeOtherTabs'),
      run: () => closeOtherTabs(activeTab),
    }] : []),
    ...(tabsRightOf(tabs, activeTab).length ? [{
      id: 'n-close-right', group: 'nav' as const, label: t('palette.closeTabsToRight'), icon: 'close' as const, hint: bind('closeTabsToRight'),
      run: () => closeTabsToRight(activeTab),
    }] : []),
    // owner-followups-1015 #06（A22「溢出」）：上 / 下一个页签，只有一个页签时不列。
    ...(adjacentTab(tabs, activeTab, 1) ? [
      { id: 'n-next-tab', group: 'nav' as const, label: t('palette.nextTab'), icon: 'chevron-right' as const, hint: bind('nextTab'), run: () => stepTab(1) },
      { id: 'n-prev-tab', group: 'nav' as const, label: t('palette.prevTab'), icon: 'chevron-left' as const, hint: bind('prevTab'), run: () => stepTab(-1) },
    ] : []),
    ...(tabPath(tabs.find((x) => x.id === activeTab)) ? [{
      id: 'n-copy-path', group: 'nav' as const, label: t('palette.copyPath'), icon: 'copy' as const, hint: bind('copyPath'),
      run: () => copyTabPath(activeTab),
    }] : []),
  ]
}

/** owner-followups-1015 #07（A16b 文件树行）：插入到输入条 / 在访达中显示，同样作用于当前文件 / 产物页签。 */
export function pathItems(t: TFunction, bind: Bind, tabs: WorkTab[], activeTab: string | null): Item[] {
  if (!activeTab || !tabPath(tabs.find((x) => x.id === activeTab))) return []
  return [{
    id: 'n-insert-path', group: 'nav', label: t('palette.insertPath'), icon: 'message', hint: bind('insertPath'),
    run: () => { const p = tabPath(tabs.find((x) => x.id === activeTab)); const ins = p && treeInsertPath({ path: p, kind: 'file' }); if (ins) insertPathToComposer(ins) },
  }, {
    id: 'n-reveal-path', group: 'nav', label: t(revealLabelKey()), icon: 'folder', hint: bind('revealPath'),
    run: () => { const p = tabPath(tabs.find((x) => x.id === activeTab)); if (p) void revealPath(p) },
  }]
}

type OpenTab = Parameters<ReturnType<typeof useUiStore.getState>['openTab']>[0]

/** 快速打开：产物（file-text + 等宽路径 +「产物」）、Agent（头像 + 角色名 +「Agent」）。 */
export function quickOpenItems(
  t: TFunction, artifacts: { path: string; version: number }[], team: { id: string; role: string }[],
  openTab: (tab: OpenTab) => void,
): Item[] {
  return [
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
}

/** owner-followups-1015 #08（A16b「定位」）：作用于打开面板前焦点所在成员行的动作。 */
export function memberItems(t: TFunction, bind: Bind, member: { id: string; role: string } | undefined): Item[] {
  if (!member) return []
  const role = member.role
  return [
    // #29：「打开详情」补进面板，与同菜单其他动作一致。
    { id: 'c-open-member-detail', group: 'cmd', label: t('palette.openMemberDetail', { role }), icon: 'agent',
      hint: bind('openMemberDetail'), run: () => openMemberDetail(member.id) },
    { id: 'c-mention-member', group: 'cmd', label: t('palette.mentionMember', { role }), icon: 'message',
      hint: bind('mentionMember'), run: () => mentionMember(member.id) },
    { id: 'c-copy-member-name', group: 'cmd', label: t('palette.copyMemberName', { role }), icon: 'copy',
      hint: bind('copyMemberName'), run: () => copyMemberName(member.id) },
    // #18：「恢复默认」照列——面板是同步列表，角色有无自定义头像在执行时判（没有则提示，不发删除）。
    { id: 'c-change-member-avatar', group: 'cmd', label: t('palette.changeMemberAvatar', { role }), icon: 'image',
      hint: bind('changeMemberAvatar'), run: () => changeMemberAvatar(member.id) },
    { id: 'c-restore-member-avatar', group: 'cmd', label: t('palette.restoreMemberAvatar', { role }), icon: 'undo',
      hint: bind('restoreMemberAvatar'), run: () => void restoreMemberAvatar(member.id) },
  ]
}

/** owner-followups-1015 #08：作用于打开面板前焦点所在消息的动作。 */
export function messageItems(t: TFunction, bind: Bind, host: HTMLElement | null): Item[] {
  if (!host) return []
  return [
    { id: 'c-copy-message', group: 'cmd', label: t('palette.copyMessage'), icon: 'copy',
      hint: bind('copyMessage'), run: () => dispatchMessageAction(host, 'copy') },
    { id: 'c-quote-message', group: 'cmd', label: t('palette.quoteMessage'), icon: 'message',
      hint: bind('quoteMessage'), run: () => dispatchMessageAction(host, 'quote') },
    ...(messageOnRail(host) ? [{ id: 'c-locate-message', group: 'nav' as const, label: t('palette.locateInRail'), icon: 'crosshair' as const,
      hint: bind('locateInRail'), run: () => dispatchMessageAction(host, 'locate') }] : []),
  ]
}

/** #31「打开方式」视图：目标（文件 / 目录）的编辑器在前，整个项目的在后；名称不翻译。 */
export function openWithItems(t: TFunction, target: string | null, targetApps: OpenWithState, projectApps: OpenWithState): Item[] {
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
export function openWithEmptyKey(a: OpenWithState, b: OpenWithState): string {
  if (a.kind === 'error' || b.kind === 'error') return 'ctx.openWithAppsError'
  if (b.kind === 'loading') return 'ctx.openWithAppsLoading'
  return 'ctx.openWithNoApps'
}

/** #13 推送项：有上游（或还没读到目标）= 一项「推送」，描述写目标远程；
 *  没有上游 = 每个远程一项「推送到 X」，由负责人选；一个远程都没有 = 一项，执行时说明原因。 */
export function pushItems(
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

