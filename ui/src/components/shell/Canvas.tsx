// 画布 Canvas（components B7，canvas#artifact / canvas#file / workbench#resize；interaction §1 §6 §12）。
// ADR 0088：中栏 Tab 容器改为右侧画布，时间线不再是页签（主区常驻对话）。页签 id 沿用旧 WorkTab 规则。
// 页签栏 48 高：A22 页签列 + 右侧「展开到全宽」/ 全宽时「与对话并排」+「关闭画布」；全宽时最左出固定「对话」页签。
// 内容（产物 / 文件 / Agent / diff）原样装进来，换皮在票 09。
//
// 关着的画布不卸载（hidden）：未保存的文件草稿要留到离开确认能看见（Benchmark I1 同理），
// 所以 App 只要有页签就渲染本组件，用 open 控制显隐。
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore, type WorkTab } from '../../store'
import { parseUnifiedDiff } from '../../diff'
import { Icon } from '../Icon'
import { FileTypeIcon } from '../FileTypeIcon'
import { ArtifactTab } from '../ArtifactTab'
import { AgentTab } from '../AgentTab'
import { DiffView } from '../DiffView'
import { FileEditor } from '../FileEditor'
import { FileTree } from '../FileTree'
import { ArtifactList } from './ArtifactList'
import { FileContextMenu, useProjectFileTarget } from '../FileMenu'
import type { ArtifactRow } from '../../gen/ArtifactRow'
// 页签栏（含票 09 全宽状态槽、#06 溢出）在 CanvasTabs.tsx。
import { CanvasTabs, type CanvasMode } from './CanvasTabs'
import { TREE, readTreeWidth, writeTreeWidth } from '../../layout'
import { ResizeHandle } from '@/components/ui/resize-handle'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export type { CanvasMode }

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

/** 空态最近产物一行；右键 = 共用文件菜单（A16b，#32，2026-10-21 负责人裁决；待恢复的只给「打开」，同产物清单）。 */
function RecentArtifact({ a }: { a: ArtifactRow }) {
  const open = () => useUiStore.getState().openTab({ id: `art:${a.path}`, kind: 'artifact', title: a.path, path: a.path })
  const target = useProjectFileTarget(a.materialization === 'pending_recovery' ? null : a.path, { onOpen: open })
  return (
    <FileContextMenu {...target}>
      <button type="button" data-canvas-recent className="hx-focus flex h-8 w-full cursor-pointer items-center gap-2 rounded-[var(--r-sm)] px-2 text-[13px] text-ink-2 hover:bg-hover hover:text-ink"
        onClick={open}>
        <FileTypeIcon name={a.path.split('/').pop() ?? a.path} kind="file" size={14} />
        <span className="truncate font-mono text-[12.5px]">{a.path}</span>
      </button>
    </FileContextMenu>
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
          {recent.map((a) => <RecentArtifact key={a.id} a={a} />)}
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
