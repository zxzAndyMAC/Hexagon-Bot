// 画布页签栏（components B7 页签栏 48 高 + A22 页签；owner-followups-1015 #06 从 Canvas.tsx 拆出）。
// A22「溢出」（2026-10-17 负责人裁决）：页签先压到最小宽 96（名省略，title 全名）→ 仍放不下则页签列横向滚动，
// 激活页签变化时滚入视区，竖向滚轮转横向；右侧常驻「⌄ 全部页签」→ A16 菜单（当前 ✓，>10 条顶部出搜索）；
// 固定页签（产物 / 项目文件）钉左，不在滚动容器里；⌘⇧] / ⌘⇧[ 按可见顺序切换（App 全局键 + tabActions.stepTab）。
// 起因：页签多了以后 overflow-hidden 把新开的文件裁在右边看不见（spec「页签多了看不到新的」）。
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode, type WheelEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { isPinnedTab, useUiStore, type WorkTab } from '../../store'
import { bindingFor, formatBinding, isMac } from '../../keymap'
import { Icon, type IconName } from '../Icon'
import { FileTypeIcon } from '../FileTypeIcon'
import { ComputerControlStatus } from '../ComputerControlStatus'
import { ManagedPill } from '../ManagedPill'
import { IconButton, iconButtonVariants } from '@/components/ui/icon-button'
import { Input } from '@/components/ui/input'
import { Dot } from '@/components/ui/dot'
import { cn } from '@/lib/utils'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from '@/components/ui/context-menu'
import { OpenWithSubmenu } from '../OpenWithMenu'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuFooter, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { closeOtherTabs, closeTabsToRight, copyTabPath, otherTabs, pinnedFirst, revealDelta, tabPath, tabsRightOf } from '../../tabActions'

export type CanvasMode = 'side' | 'full' | 'sheet'

/** 「全部页签」菜单超过这么多条才出搜索（A22「溢出」）。 */
const SEARCH_AFTER = 10

const KIND_ICON: Record<WorkTab['kind'], IconName> = {
  artifact: 'file-text', diff: 'diff', agent: 'agent', file: 'file', artifacts: 'layers', files: 'folder',
}

function TabIcon({ tab }: { tab: WorkTab }) {
  if (tab.kind === 'file') return <FileTypeIcon name={(tab.path ?? tab.title).split('/').pop() ?? tab.title} kind="file" size={14} />
  return <Icon name={KIND_ICON[tab.kind]} size={14} />
}

function useTabTitle() {
  const { t } = useTranslation()
  return (tab: WorkTab) =>
    tab.kind === 'artifacts' ? t('sidebar.artifacts')
    : tab.kind === 'files' ? t('sidebar.files')
    : tab.kind === 'agent' ? (tab.role ?? tab.title)
    : tab.kind === 'artifact' || tab.kind === 'file' ? (tab.path ?? tab.title).split('/').pop() ?? tab.title
    : tab.title
}

// A22「溢出」第一步：flex 0 1 auto + 最小宽 96，放不下先一起压窄（名省略），压到 96 才轮到横向滚动。
const tabClass = (on: boolean) => cn(
  'hx-press hx-focus inline-flex h-[30px] min-w-[96px] max-w-[200px] flex-[0_1_auto] cursor-pointer items-center gap-1.5 rounded-[var(--r-sm)] pr-1 pl-2.5 text-[12.5px] whitespace-nowrap',
  on ? 'bg-hover-2 font-medium text-ink' : 'text-ink-2 hover:bg-hover',
)

/** ←→ 在页签间移动焦点（A22 聚焦行）；固定组与滚动区同属一个 tablist，按可见顺序走。 */
function arrowTabs(e: KeyboardEvent<HTMLDivElement>) {
  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
  const tabs = [...e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')]
  const i = tabs.indexOf(document.activeElement as HTMLElement)
  if (i < 0) return
  e.preventDefault()
  const next = tabs[(i + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length]
  next.focus()
  next.click()
}

/** 画布页签右键菜单（A16b 第一批）：关闭 · 关闭其他 · 关闭右侧 · 复制路径 · 打开方式 ▸（后两项仅文件 / 产物页签；#31）。 */
function TabContextMenu({ tab, tabs, children }: { tab: WorkTab; tabs: WorkTab[]; children: ReactNode }) {
  const { t } = useTranslation()
  const path = tabPath(tab)
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem data-ctx-item="close" action="closeTab" onSelect={() => useUiStore.getState().closeTab(tab.id)}>
          {t('ctx.close')}
        </ContextMenuItem>
        <ContextMenuItem data-ctx-item="closeOthers" action="closeOtherTabs"
          reason={otherTabs(tabs, tab.id).length ? undefined : t('ctx.noOtherTabs')} onSelect={() => closeOtherTabs(tab.id)}>
          {t('ctx.closeOthers')}
        </ContextMenuItem>
        <ContextMenuItem data-ctx-item="closeRight" action="closeTabsToRight"
          reason={tabsRightOf(tabs, tab.id).length ? undefined : t('ctx.noTabsRight')} onSelect={() => closeTabsToRight(tab.id)}>
          {t('ctx.closeRight')}
        </ContextMenuItem>
        {path && (
          <>
            <ContextMenuSeparator />
            <ContextMenuItem data-ctx-item="copyPath" action="copyPath" onSelect={() => void copyTabPath(tab.id)}>
              {t('ctx.copyPath')}
            </ContextMenuItem>
            <OpenWithSubmenu path={path} />
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  )
}

function TabItem({ tab, tabs, on, name, dirty }: { tab: WorkTab; tabs: WorkTab[]; on: boolean; name: string; dirty: boolean }) {
  const { t } = useTranslation()
  const st = useUiStore.getState
  return (
    <TabContextMenu tab={tab} tabs={tabs}>
      <div className={tabClass(on)} data-canvas-tab={tab.id}
        onAuxClick={(e) => { if (e.button === 1) { e.preventDefault(); st().closeTab(tab.id) } }}>
        <button type="button" role="tab" aria-selected={on} tabIndex={on ? 0 : -1} title={tab.path ?? name}
          className="flex min-w-0 cursor-pointer items-center gap-1.5 outline-none"
          // A16b：macOS ctrl+点击由系统当右键（发 contextmenu 开菜单），不能顺带切页签（「右键不改选中」）。
          onClick={(e) => { if (!(isMac && e.ctrlKey)) st().setActiveTab(tab.id) }}>
          <TabIcon tab={tab} />
          <span className="truncate">{name}</span>
          {dirty && <Dot tone="warn" data-unsaved />}
        </button>
        <IconButton size="sm" label={t('canvas.closeTab', { name })} action="closeTab" className="opacity-50 hover:opacity-100"
          onClick={(e) => { e.stopPropagation(); st().closeTab(tab.id) }}>
          <Icon name="close" size={12} />
        </IconButton>
      </div>
    </TabContextMenu>
  )
}

/** 搜索框：打开即聚焦；字符键不冒泡给 radix 菜单（否则 typeahead 把焦点抢到匹配行），↓ 进第一行。 */
function TabSearch({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useTranslation()
  const ref = useRef<HTMLInputElement>(null)
  // 子组件 effect 先于 radix FocusScope 的挂载聚焦执行；焦点已在容器内时 FocusScope 不再抢。
  useEffect(() => { ref.current?.focus() }, [])
  return (
    <div className="p-1 pb-1.5">
      <Input ref={ref} data-all-tabs-search value={value} aria-label={t('canvas.searchTabs')} placeholder={t('canvas.searchTabs')}
        leading={<Icon name="search" size={14} />}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape' || e.key === 'Tab') return
          e.stopPropagation()
          if (e.key !== 'ArrowDown') return
          e.preventDefault()
          e.currentTarget.closest('[data-slot="dropdown-menu-content"]')?.querySelector<HTMLElement>('[data-all-tabs-item]')?.focus()
        }} />
    </div>
  )
}

/** 「⌄ 全部页签」（A22「溢出」）：A16 菜单列全部页签（可见顺序，当前 ✓），>10 条顶部出搜索；底部写上 / 下一个页签的绑定。 */
function AllTabsMenu({ tabs, active, title }: { tabs: WorkTab[]; active: string | null; title: (tab: WorkTab) => string }) {
  const { t } = useTranslation()
  const [query, setQuery] = useState('')
  const ordered = pinnedFirst(tabs)
  const searchable = ordered.length > SEARCH_AFTER
  const q = query.trim().toLowerCase()
  const shown = searchable && q
    ? ordered.filter((tab) => title(tab).toLowerCase().includes(q) || (tab.path ?? '').toLowerCase().includes(q))
    : ordered
  const label = t('canvas.allTabs')
  return (
    <DropdownMenu onOpenChange={(open) => { if (!open) setQuery('') }}>
      <Tooltip>
        <TooltipTrigger asChild>
          <DropdownMenuTrigger asChild>
            <button type="button" aria-label={label} data-canvas-all-tabs className={iconButtonVariants({ size: 'sm' })}>
              <Icon name="chevron-down" size={14} />
            </button>
          </DropdownMenuTrigger>
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="end" className="w-[280px]">
        {searchable && <TabSearch value={query} onChange={setQuery} />}
        <DropdownMenuRadioGroup value={active ?? ''} className="max-h-[min(360px,60vh)] overflow-y-auto">
          {shown.map((tab) => (
            <DropdownMenuRadioItem key={tab.id} value={tab.id} data-all-tabs-item={tab.id} title={tab.path ?? title(tab)}
              onSelect={() => useUiStore.getState().setActiveTab(tab.id)}>
              <TabIcon tab={tab} />
              <span className="min-w-0 truncate">{title(tab)}</span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        {shown.length === 0 && <div className="flex h-8 items-center px-[9px] text-[13px] text-ink-3">{t('canvas.noTabMatch')}</div>}
        <DropdownMenuFooter>
          {t('canvas.tabCycleHint', { prev: formatBinding(bindingFor('prevTab')), next: formatBinding(bindingFor('nextTab')) })}
        </DropdownMenuFooter>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

export function CanvasTabs({ mode, narrowLead }: { mode: CanvasMode; narrowLead: boolean }) {
  const { t } = useTranslation()
  const tabs = useUiStore((s) => s.tabs)
  const active = useUiStore((s) => s.activeTab)
  const fileEdits = useUiStore((s) => s.fileEdits)
  const title = useTabTitle()
  const st = useUiStore.getState
  const scroller = useRef<HTMLDivElement>(null)
  const dirty = (tab: WorkTab) => tab.kind === 'file' && Object.values(fileEdits).some((e) => e.dirty && e.path === tab.path)
  const pinned = tabs.filter(isPinnedTab)
  const rest = tabs.filter((tab) => !isPinnedTab(tab))
  // A22「溢出」：激活页签（含新打开的）滚入视区。只动页签列自己的 scrollLeft，不用 scrollIntoView——
  // 它会连带滚动画布 / 主区等祖先容器。
  useLayoutEffect(() => {
    const view = scroller.current
    const node = active ? view?.querySelector<HTMLElement>(`[data-canvas-tab="${CSS.escape(active)}"]`) : null
    if (!view || !node) return
    const delta = revealDelta(view.getBoundingClientRect(), node.getBoundingClientRect())
    if (delta) view.scrollLeft += delta
  }, [active, tabs.length])
  // 竖向滚轮转横向（触控板横扫本来就是横向，不动它）。
  const onWheel = (e: WheelEvent<HTMLDivElement>) => {
    if (Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return
    e.currentTarget.scrollLeft += e.deltaY
  }
  const item = (tab: WorkTab) => (
    <TabItem key={tab.id} tab={tab} tabs={tabs} on={tab.id === active} name={title(tab)} dirty={dirty(tab)} />
  )
  return (
    <div
      data-canvas-tabs
      className={cn('flex h-12 flex-none items-center gap-1 border-b border-line-soft pr-2 pl-2.5', isMac && narrowLead && 'pl-7')}
    >
      {mode === 'full' && (
        <button type="button" data-canvas-conversation className={cn(tabClass(false), 'min-w-0 flex-none')} onClick={() => st().showConversation()}>
          <Icon name="message" size={14} />{t('canvas.conversation')}
        </button>
      )}
      <div role="tablist" aria-label={t('canvas.tabs')} onKeyDown={arrowTabs} className="flex min-w-0 items-center gap-1">
        {pinned.length > 0 && <div data-canvas-tab-pinned className="flex flex-none items-center gap-1">{pinned.map(item)}</div>}
        <div ref={scroller} data-canvas-tab-scroller onWheel={onWheel}
          className="flex min-w-0 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {rest.map(item)}
        </div>
      </div>
      <span data-tauri-drag-region className="flex-1 self-stretch" />
      {/* 票 09（协调裁决：票 03 偏差 4）：全宽时头部随对话收起，电脑控制状态与托管胶囊挪到页签栏右侧，
          Agent 操控电脑、托管开着时负责人在任何主区视图都看得见。Header 在全宽时不渲染这两件，免得双挂
          （ManagedPill 注册全局暂停 / 关闭键，挂两份会一键执行两次）。 */}
      {mode === 'full' && (
        <div data-canvas-status className="flex flex-none items-center gap-1.5 pr-1">
          <ComputerControlStatus />
          <ManagedPill />
        </div>
      )}
      {tabs.length > 0 && <AllTabsMenu tabs={tabs} active={active} title={title} />}
      {mode === 'full' ? (
        <IconButton size="sm" label={t('canvas.sideBySide')} action="canvasFullWidth" data-canvas-side-by-side onClick={() => st().toggleCanvasFull()}>
          <Icon name="columns" size={14} />
        </IconButton>
      ) : (
        <IconButton size="sm" label={t('canvas.expand')} action="canvasFullWidth" data-canvas-expand onClick={() => st().toggleCanvasFull()}>
          <Icon name="expand" size={14} />
        </IconButton>
      )}
      <IconButton size="sm" label={t('canvas.close')} action="toggleCanvas" data-canvas-close onClick={() => st().setCanvasOpen(false)}>
        <Icon name="close" size={14} />
      </IconButton>
    </div>
  )
}
