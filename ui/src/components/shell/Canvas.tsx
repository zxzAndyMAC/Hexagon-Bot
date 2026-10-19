// 画布 Canvas（components B7，canvas#artifact / canvas#file / workbench#resize；interaction §1 §6 §12）。
// ADR 0088：中栏 Tab 容器改为右侧画布，时间线不再是页签（主区常驻对话）。页签 id 沿用旧 WorkTab 规则。
// 页签栏 48 高：A22 页签列 + 右侧「展开到全宽」/ 全宽时「与对话并排」+「关闭画布」；全宽时最左出固定「对话」页签。
// 内容（产物 / 文件 / Agent / diff）原样装进来，换皮在票 09。
//
// 关着的画布不卸载（hidden）：未保存的文件草稿要留到离开确认能看见（Benchmark I1 同理），
// 所以 App 只要有页签就渲染本组件，用 open 控制显隐。
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore, type WorkTab } from '../../store'
import { isMac } from '../../keymap'
import { parseUnifiedDiff } from '../../diff'
import { Icon, type IconName } from '../Icon'
import { FileTypeIcon } from '../FileTypeIcon'
import { ArtifactTab } from '../ArtifactTab'
import { AgentTab } from '../AgentTab'
import { DiffView } from '../DiffView'
import { FileEditor } from '../FileEditor'
import { FileTree } from '../FileTree'
import { ArtifactList } from './ArtifactList'
// 票 09（协调裁决：票 03 偏差 4）：全宽时头部随对话收起，电脑控制状态与托管胶囊挪到页签栏右侧，
// Agent 操控电脑、托管开着时负责人在任何主区视图都看得见。Header 在全宽时不渲染这两件，免得双挂
// （ManagedPill 注册全局暂停 / 关闭键，挂两份会一键执行两次）。
import { ComputerControlStatus } from '../ComputerControlStatus'
import { ManagedPill } from '../ManagedPill'
import { TREE, readTreeWidth, writeTreeWidth } from '../../layout'
import { ResizeHandle } from '@/components/ui/resize-handle'
import { IconButton } from '@/components/ui/icon-button'
import { Button } from '@/components/ui/button'
import { Dot } from '@/components/ui/dot'
import { cn } from '@/lib/utils'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from '@/components/ui/context-menu'
import { closeOtherTabs, closeTabsToRight, copyTabPath, otherTabs, tabPath, tabsRightOf } from '../../tabActions'

export type CanvasMode = 'side' | 'full' | 'sheet'

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

const tabClass = (on: boolean) => cn(
  'hx-press hx-focus inline-flex h-[30px] min-w-0 max-w-[200px] flex-none cursor-pointer items-center gap-1.5 rounded-[var(--r-sm)] pr-1 pl-2.5 text-[12.5px] whitespace-nowrap',
  on ? 'bg-hover-2 font-medium text-ink' : 'text-ink-2 hover:bg-hover',
)

/** ←→ 在页签间移动焦点（A22 聚焦行）。 */
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

/** 画布页签右键菜单（A16b 第一批）：关闭 · 关闭其他 · 关闭右侧 · 复制路径（仅文件 / 产物页签）。 */
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
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  )
}

function CanvasTabs({ mode, narrowLead }: { mode: CanvasMode; narrowLead: boolean }) {
  const { t } = useTranslation()
  const tabs = useUiStore((s) => s.tabs)
  const active = useUiStore((s) => s.activeTab)
  const fileEdits = useUiStore((s) => s.fileEdits)
  const title = useTabTitle()
  const st = useUiStore.getState
  const dirty = (tab: WorkTab) => tab.kind === 'file' && Object.values(fileEdits).some((e) => e.dirty && e.path === tab.path)
  return (
    <div
      data-canvas-tabs
      className={cn('flex h-12 flex-none items-center gap-1 border-b border-line-soft pr-2 pl-2.5', isMac && narrowLead && 'pl-7')}
    >
      {mode === 'full' && (
        <button type="button" data-canvas-conversation className={tabClass(false)} onClick={() => st().showConversation()}>
          <Icon name="message" size={14} />{t('canvas.conversation')}
        </button>
      )}
      <div role="tablist" aria-label={t('canvas.tabs')} onKeyDown={arrowTabs} className="flex min-w-0 items-center gap-1 overflow-hidden">
        {tabs.map((tab) => {
          const on = tab.id === active
          const name = title(tab)
          return (
            <TabContextMenu key={tab.id} tab={tab} tabs={tabs}>
            <div className={tabClass(on)} data-canvas-tab={tab.id}
              onAuxClick={(e) => { if (e.button === 1) { e.preventDefault(); st().closeTab(tab.id) } }}>
              <button type="button" role="tab" aria-selected={on} tabIndex={on ? 0 : -1} title={tab.path ?? name}
                className="flex min-w-0 cursor-pointer items-center gap-1.5 outline-none"
                // A16b：macOS ctrl+点击由系统当右键（发 contextmenu 开菜单），不能顺带切页签（「右键不改选中」）。
                onClick={(e) => { if (!(isMac && e.ctrlKey)) st().setActiveTab(tab.id) }}>
                <TabIcon tab={tab} />
                <span className="truncate">{name}</span>
                {dirty(tab) && <Dot tone="warn" data-unsaved />}
              </button>
              <IconButton size="sm" label={t('canvas.closeTab', { name })} action="closeTab" className="opacity-50 hover:opacity-100"
                onClick={(e) => { e.stopPropagation(); st().closeTab(tab.id) }}>
                <Icon name="close" size={12} />
              </IconButton>
            </div>
            </TabContextMenu>
          )
        })}
      </div>
      <span data-tauri-drag-region className="flex-1 self-stretch" />
      {mode === 'full' && (
        <div data-canvas-status className="flex flex-none items-center gap-1.5 pr-1">
          <ComputerControlStatus />
          <ManagedPill />
        </div>
      )}
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

function TabBody({ tab }: { tab: WorkTab }) {
  const { t } = useTranslation()
  switch (tab.kind) {
    case 'file':
      return <FileEditor path={tab.path!} />
    case 'artifact':
      return <ArtifactTab path={tab.path!} />
    case 'agent':
      return <AgentTab agentId={tab.agentId!} />
    case 'artifacts':
      return <ArtifactList />
    case 'files':
      return <div className="grid flex-1 place-items-center text-[13px] text-ink-3">{t('canvas.pickFile')}</div>
    case 'diff':
      return (
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="row-line mono" style={{ padding: '8px 14px', fontSize: 12, fontWeight: 560, display: 'flex', alignItems: 'center', gap: 6 }}>
            <Icon name="diff" size={12} /> {tab.title}
          </div>
          <DiffView ops={parseUnifiedDiff(tab.patchText ?? '')} />
        </div>
      )
  }
}

/** 画布内文件树（D14）：宽 210，160–360 可拖，停在下限不吸附；双击还原；宽度存 `hexagon.layout.tree`。 */
function TreePane() {
  const { t } = useTranslation()
  const [width, setWidth] = useState(() => readTreeWidth(localStorage))
  const commit = (w: number) => { setWidth(w); writeTreeWidth(localStorage, w) }
  return (
    <>
      <div data-canvas-tree style={{ width }} className="flex min-h-0 flex-none flex-col overflow-hidden">
        <FileTree />
      </div>
      <ResizeHandle
        label={t('canvas.treeWidth')}
        value={width}
        limits={{ min: TREE.min, max: TREE.max }}
        tipTitle={t('canvas.treeWidth')}
        tipExtra={t('layout.resetHint')}
        onDrag={(r) => setWidth(r.width)}
        onCommit={(r) => commit(r.width)}
        onDoubleClick={() => commit(TREE.default)}
      />
    </>
  )
}

/** 空态（A19，interaction §6 补设计）：说明为什么空 + 一个动作（打开项目文件）+ 最近产物。 */
function CanvasEmpty() {
  const { t } = useTranslation()
  const artifacts = useUiStore((s) => s.artifacts)
  const st = useUiStore.getState
  const recent = artifacts.slice(-5).reverse()
  return (
    <div data-canvas-empty className="flex flex-1 flex-col items-center justify-center px-5 py-7 text-center">
      <span className="grid size-11 place-items-center rounded-[12px] bg-canvas text-ink-2 shadow-[inset_0_0_0_1px_var(--line)]">
        <Icon name="layers" size={20} />
      </span>
      <div className="mt-3 text-[14px] font-semibold text-ink">{t('canvas.empty')}</div>
      <p className="mt-1 max-w-[280px] text-[13px] text-ink-3">{t('canvas.emptyHint')}</p>
      <Button size="sm" className="mt-3" onClick={() => st().openCanvas('files')}>
        <Icon name="folder" size={14} />{t('sidebar.files')}
      </Button>
      {recent.length > 0 && (
        <div className="mt-6 w-full max-w-[320px] text-left">
          <div className="mb-1 px-2 text-[12px] text-ink-3">{t('canvas.recent')}</div>
          {recent.map((a) => (
            <button key={a.id} type="button" className="hx-focus flex h-8 w-full cursor-pointer items-center gap-2 rounded-[var(--r-sm)] px-2 text-[13px] text-ink-2 hover:bg-hover hover:text-ink"
              onClick={() => st().openTab({ id: `art:${a.path}`, kind: 'artifact', title: a.path, path: a.path })}>
              <FileTypeIcon name={a.path.split('/').pop() ?? a.path} kind="file" size={14} />
              <span className="truncate font-mono text-[12.5px]">{a.path}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

export function Canvas({ open, mode, width, narrowLead = false }: {
  open: boolean
  mode: CanvasMode
  width: number
  /** macOS 红绿灯让位：侧栏不是展开态时全宽画布的页签栏左侧多让 28px（同头部 B5）。 */
  narrowLead?: boolean
}) {
  const { t } = useTranslation()
  const tabs = useUiStore((s) => s.tabs)
  const active = useUiStore((s) => s.activeTab)
  const activeKind = tabs.find((x) => x.id === active)?.kind
  const root = useRef<HTMLElement>(null)
  // 焦点（B7 聚焦行 / interaction §12）：打开时焦点进当前页签，关闭时回到触发处。
  const opener = useRef<Element | null>(null)
  useEffect(() => {
    if (!open) return
    const aside = root.current
    opener.current = document.activeElement
    aside?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus()
    return () => {
      const back = opener.current
      opener.current = null
      // 焦点还在画布里（或画布刚被隐藏、焦点掉到 body）才还给触发处；负责人已经点去别处就不抢。
      const lost = document.activeElement === document.body || aside?.contains(document.activeElement)
      if (back instanceof HTMLElement && back.isConnected && lost) back.focus()
    }
  }, [open])
  // 焦点停在页签列上时跟着当前页签走（从清单打开新页签后，焦点环不留在旧页签上）
  useEffect(() => {
    const focused = document.activeElement
    if (!open || !(focused instanceof HTMLElement) || focused.getAttribute('role') !== 'tab' || !root.current?.contains(focused)) return
    root.current.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus()
  }, [open, active])
  const showTree = activeKind === 'file' || activeKind === 'files'
  return (
    <aside
      ref={root}
      hidden={!open}
      data-canvas={mode}
      aria-label={t('canvas.label')}
      style={mode === 'full' ? undefined : { width }}
      className={cn(
        'flex min-h-0 min-w-0 flex-col bg-[var(--app-main)] dark:bg-page',
        mode === 'full' && 'flex-1',
        mode === 'side' && 'flex-none',
        // Sheet（interaction §1 让位最后一步）：浮在对话上，A8 overlay 阴影
        mode === 'sheet' && 'absolute inset-y-0 right-0 z-[18] border-l border-line shadow-[var(--shadow-overlay)]',
      )}
    >
      <CanvasTabs mode={mode} narrowLead={narrowLead && mode === 'full'} />
      {tabs.length === 0 ? <CanvasEmpty /> : (
        <div className="flex min-h-0 flex-1">
          {/* canvas#file：文件树在编辑器左侧，所有文件页签共用一棵（只挂一次，免得树命令被消费两遍） */}
          {showTree && <TreePane />}
          <div className="flex min-h-0 min-w-0 flex-1 flex-col">
            {tabs.map((tab) => (
              <div key={tab.id} role="tabpanel" hidden={tab.id !== active} className="flex min-h-0 flex-1 flex-col">
                <TabBody tab={tab} />
              </div>
            ))}
          </div>
        </div>
      )}
    </aside>
  )
}
