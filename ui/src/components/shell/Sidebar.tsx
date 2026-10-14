// 左侧栏 Sidebar（components B1；workbench#running、workbench#collapsed、index#ia；ADR 0088）。
// 自上而下：窗口拖拽栏 44（收起 ibtn）→ 项目块 → 导航（搜索与命令 ⌘K / 待决 + 橙徽标 / 产物 + 计数 / 项目文件）
// → 滚动区：「流程」阶段列表 B2、「团队」成员行 B3 → 底部（负责人 + 用量入口 B4 + 设置）。
// 收起为 56 宽图标列：hexmark、搜索、待决（红点）、产物、文件、团队头像列、设置；悬停 tooltip 带键位。
// 整栏是 nav landmark，方向键在可聚焦项之间移动（B1 聚焦行）。
//
// 待决徽标只计负责人事项：宿主的 pending 读已排除托管代办的卡（owner_pending，ADR 0086），
// 加上待复审的提案行；托管正在处理的件数不进这里（在托管胶囊里）。
import type * as React from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { isMac } from '../../keymap'
import { Icon, type IconName } from '../Icon'
import { ProjectBlock, Hexmark } from './ProjectBlock'
import { StageList } from './StageList'
import { TeamList } from './TeamList'
import { useAddRole, useWorkingAgents } from './teamHooks'
import { UsageButton } from './UsageEntry'
import { Avatar } from '@/components/ui/avatar'
import { IconButton } from '@/components/ui/icon-button'
import { ShortcutKbd } from '@/components/ui/kbd'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { bindingFor, type ActionId } from '../../keymap'
import { cn } from '@/lib/utils'

export type SidebarProps = {
  onSearch: () => void
  onSettings: () => void
  onProjectClosed: () => void
  /** 窗口过窄被逼成图标列（票 03，interaction §1 让位顺序第二步）；不改 `hexagon.layout.side`。 */
  forceCollapsed?: boolean
}

/** 负责人事项数：待决卡 + 待复审提案（与待决界面同一口径）。 */
function useOwnerPendingCount() {
  return useUiStore((s) => s.pending.length + s.reviewRows.length)
}

/** B1「整栏为 nav landmark，方向键在项间移动」：↑↓ 在侧栏内可聚焦控件之间移动焦点。 */
function arrowNav(e: React.KeyboardEvent<HTMLElement>) {
  if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
  const target = e.target as HTMLElement
  if (target.closest('input, textarea, select, [role="separator"]')) return
  const items = [...e.currentTarget.querySelectorAll<HTMLElement>('button:not([aria-disabled="true"]), a[href]')]
  const i = items.indexOf(target)
  if (i < 0) return
  e.preventDefault()
  items[Math.min(items.length - 1, Math.max(0, i + (e.key === 'ArrowDown' ? 1 : -1)))]?.focus()
}

function NavItem({ icon, label, on, onClick, trailing, data }: {
  icon: IconName; label: string; on?: boolean; onClick: () => void; trailing?: React.ReactNode; data?: string
}) {
  return (
    <button
      type="button"
      data-nav={data}
      aria-current={on || undefined}
      onClick={onClick}
      className={cn(
        'hx-press hx-focus flex h-8 w-full cursor-pointer items-center gap-[9px] rounded-[var(--r-sm)] px-2 text-left text-[13px] text-ink-2 live:hover:bg-hover live:hover:text-ink',
        on && 'bg-hover-2 font-medium text-ink',
      )}
    >
      <Icon name={icon} size={16} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {trailing}
    </button>
  )
}

function PendingBadge({ count }: { count: number }) {
  return (
    <span data-pending-count={count} className="grid h-[18px] min-w-[18px] place-items-center rounded-full bg-orange px-[5px] text-[11px] font-semibold text-white">
      {count}
    </span>
  )
}

function SectionTitle({ label, action }: { label: string; action?: React.ReactNode }) {
  return (
    <div className="mx-[18px] mt-3.5 mb-1 flex items-center gap-1 text-[12px] text-ink-3">
      <Icon name="chevron-down" size={12} />
      <span>{label}</span>
      <span className="flex-1" />
      {action}
    </div>
  )
}

/** 窗口拖拽栏（44 高）：macOS 红绿灯落在这里（overlay 标题栏），非 macOS 不留空档。 */
function WinBar({ children, center }: { children?: React.ReactNode; center?: boolean }) {
  return (
    <div
      data-tauri-drag-region
      className={cn('flex h-11 flex-none items-center', center ? 'justify-center' : 'justify-end px-3.5', isMac && !center && 'pl-[84px]')}
    >
      {children}
    </div>
  )
}

function StatusError() {
  const { t } = useTranslation()
  const failed = useUiStore((s) => s.statusError)
  if (!failed) return null
  return (
    <div role="status" data-status-error className="mx-2.5 mb-1 flex items-center gap-1.5 text-[12px] text-red">
      <Icon name="warn" size={12} />
      <span className="flex-1">{t('sidebar.stateFailed')}</span>
      <button type="button" className="hx-focus cursor-pointer rounded-[6px] px-1.5 text-ink-2 hover:bg-hover hover:text-ink"
        onClick={() => void useUiStore.getState().refreshFast()}>{t('sidebar.retry')}</button>
    </div>
  )
}

export function Sidebar({ onSearch, onSettings, onProjectClosed, forceCollapsed = false }: SidebarProps) {
  const { t } = useTranslation()
  const width = useUiStore((s) => s.sideLayout.width)
  const collapsed = useUiStore((s) => s.sideLayout.collapsed) || forceCollapsed
  const artifacts = useUiStore((s) => s.artifacts.length)
  // 票 03：「产物」「项目文件」是画布里的清单页签；当前项 = 画布可见且停在该清单或它打开的产物 / 文件页签
  // （canvas#artifact、canvas#file 侧栏高亮）
  const canvasPane = useUiStore((s) => {
    if (!s.canvasOpen || s.usageView) return null
    const kind = s.tabs.find((x) => x.id === s.activeTab)?.kind
    return kind === 'artifacts' || kind === 'artifact' ? 'artifacts' : kind === 'files' || kind === 'file' ? 'files' : null
  })
  const pendingCount = useOwnerPendingCount()
  const statusError = useUiStore((s) => s.statusError)
  const addRole = useAddRole()
  const st = useUiStore.getState
  if (collapsed) return <SidebarRail onSearch={onSearch} onSettings={onSettings} pendingCount={statusError ? 0 : pendingCount} />
  return (
    <nav
      aria-label={t('sidebar.label')}
      data-sidebar="expanded"
      onKeyDown={arrowNav}
      style={{ width }}
      className="flex min-h-0 flex-none flex-col bg-sidebar"
    >
      <WinBar>
        <IconButton size="sm" label={t('sidebar.collapse')} action="toggleSidebar" onClick={() => st().toggleSidebar()}>
          <Icon name="panel-left" size={14} />
        </IconButton>
      </WinBar>
      <ProjectBlock onProjectClosed={onProjectClosed} />
      <div className="flex flex-col gap-px px-2.5">
        <NavItem icon="search" label={t('sidebar.search')} onClick={onSearch} data="search"
          trailing={<ShortcutKbd binding={bindingFor('commandPalette')} />} />
        <NavItem icon="inbox" label={t('sidebar.pending')} onClick={() => st().openPendingDialog()} data="pending"
          trailing={!statusError && pendingCount > 0 ? <PendingBadge count={pendingCount} /> : undefined} />
        <NavItem icon="layers" label={t('sidebar.artifacts')} data="artifacts"
          on={canvasPane === 'artifacts'} onClick={() => st().openCanvas('artifacts')}
          trailing={<span className="text-[12px] text-ink-3">{artifacts}</span>} />
        <NavItem icon="folder" label={t('sidebar.files')} data="files"
          on={canvasPane === 'files'} onClick={() => st().openCanvas('files')} />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto pb-2">
        <SectionTitle label={t('sidebar.flow')} action={
          <IconButton size="sm" label={t('sidebar.viewFlow')} onClick={() => st().setFlowDialogOpen(true)}>
            <Icon name="expand" size={12} />
          </IconButton>
        } />
        <StageList />
        <SectionTitle label={t('sidebar.team')} action={
          <IconButton size="sm" label={t('sidebar.addRole')} on={addRole.creating} onClick={addRole.toggle}>
            <Icon name="plus" size={12} />
          </IconButton>
        } />
        {addRole.form}
        <TeamList />
      </div>
      <StatusError />
      <div className="flex flex-none items-center gap-2 border-t border-line p-2.5">
        <Avatar owner name={t('sidebar.owner')} size={18} />
        <span className="flex-1 truncate text-[13px] text-ink">{t('sidebar.owner')}</span>
        <UsageButton />
        <IconButton size="sm" label={t('sidebar.settings')} action="settings" onClick={onSettings}>
          <Icon name="settings" size={14} />
        </IconButton>
      </div>
    </nav>
  )
}

function RailButton({ icon, label, action, onClick, dot, data }: {
  icon: IconName; label: string; action?: ActionId; onClick: () => void; dot?: boolean; data?: string
}) {
  return (
    <span className="relative" data-rail={data}>
      <IconButton label={label} action={action} onClick={onClick}>
        <Icon name={icon} size={16} />
      </IconButton>
      {dot && <span aria-hidden data-pending-dot className="pointer-events-none absolute top-0.5 right-0.5 size-2 rounded-full bg-orange shadow-[0_0_0_2px_var(--app-side)]" />}
    </span>
  )
}

/** 收起态图标列（workbench#collapsed）：保留待决红点和团队头像，悬停出 tooltip（带键位）。 */
function SidebarRail({ onSearch, onSettings, pendingCount }: { onSearch: () => void; onSettings: () => void; pendingCount: number }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const avatars = useUiStore((s) => s.avatars)
  const working = useWorkingAgents()
  const st = useUiStore.getState
  return (
    <nav aria-label={t('sidebar.label')} data-sidebar="collapsed" onKeyDown={arrowNav}
      className="flex w-14 min-h-0 flex-none flex-col items-center bg-sidebar">
      {/* 原型图标列的拖拽栏只放红绿灯，没有展开钮：展开走 ⌘B、单击分割线或分割线上按 ↵ */}
      <WinBar center />
      <span className="mt-1"><Hexmark /></span>
      <div className="mt-3 flex flex-col items-center gap-1">
        <RailButton icon="search" label={t('sidebar.search')} action="commandPalette" onClick={onSearch} data="search" />
        <RailButton icon="inbox" data="pending" onClick={() => st().openPendingDialog()} dot={pendingCount > 0}
          label={pendingCount > 0 ? t('sidebar.pendingCount', { count: pendingCount }) : t('sidebar.pending')} />
        <RailButton icon="layers" label={t('sidebar.artifacts')} onClick={() => st().openCanvas('artifacts')} data="artifacts" />
        <RailButton icon="folder" label={t('sidebar.files')} onClick={() => st().openCanvas('files')} data="files" />
      </div>
      <div aria-hidden className="my-3 h-px w-6 bg-line" />
      <ul aria-label={t('sidebar.team')} className="flex min-h-0 flex-1 flex-col items-center gap-1.5 overflow-y-auto">
        {team.map((m) => (
          <li key={m.id}>
            <Tooltip>
              <TooltipTrigger asChild>
                <button type="button" data-member={m.id} aria-label={m.role}
                  className={cn('hx-press hx-focus cursor-pointer rounded-[7px]', m.status === 'sleeping' && 'opacity-50')}
                  onClick={() => st().openTab({ id: `agent:${m.id}`, kind: 'agent', title: m.role, agentId: m.id, role: m.role })}>
                  <Avatar agentId={m.id} name={m.role} size={24} src={avatars[m.id]} status={m.status === 'sleeping' ? 'sleep' : 'online'} />
                </button>
              </TooltipTrigger>
              <TooltipContent side="right">
                {m.role}{m.status === 'sleeping' ? ` · ${t('sidebar.sleeping')}` : working.has(m.id) ? ` · ${t('sidebar.working')}` : ''}
              </TooltipContent>
            </Tooltip>
          </li>
        ))}
      </ul>
      <div className="mb-3">
        <IconButton label={t('sidebar.settings')} action="settings" onClick={onSettings}>
          <Icon name="settings" size={16} />
        </IconButton>
      </div>
    </nav>
  )
}
