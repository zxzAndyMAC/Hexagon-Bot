import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type RepoEntry } from '../api'
import { useUiStore } from '../store'
import { TREE_DRAG_MIME, treeDragPayload, treeInsertPath } from '../composerAtoms'
import { isMac } from '../keymap'
import { TREE_PATH_ATTR } from '../fileActions'
import { FileContextMenu } from './FileMenu'
import { FileTypeIcon } from './FileTypeIcon'
import { Icon } from './Icon'
import { Row } from './Row'
import { Button } from '@/components/ui/button'
import { Empty } from '@/components/ui/empty'
import { IconButton } from '@/components/ui/icon-button'
import { Spinner } from '@/components/ui/spinner'
import { cn } from '@/lib/utils'

function parentOf(path: string): string {
  const i = path.lastIndexOf('/')
  return i < 0 ? '' : path.slice(0, i)
}

function childName(raw: string): string | null {
  const name = raw.trim()
  if (!name || name === '.' || name === '..' || name.includes('/') || name.includes('\\') || name.includes('\0')) {
    return null
  }
  return name
}

export function FileTree() {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const pushToast = useUiStore((s) => s.pushToast)
  const projectName = useUiStore((s) => s.projectName)
  // 当前文件 = 画布当前文件页签（D14 选中行）
  const currentPath = useUiStore((s) => (s.activeTab?.startsWith('file:') ? s.activeTab.slice('file:'.length) : null))
  const [refreshing, setRefreshing] = useState(false)
  // D14 加载行：展开中的目录 chevron 换 xs 环
  const [loadingDir, setLoadingDir] = useState<string | null>(null)
  const [root, setRoot] = useState<RepoEntry[] | null>(null)
  const [kids, setKids] = useState<Record<string, RepoEntry[]>>({})
  const [anchor, setAnchor] = useState('')
  const [creating, setCreating] = useState<'file' | 'dir' | null>(() => {
    const action = useUiStore.getState().fileTreeReq?.action
    if (action === 'new-file') return 'file'
    if (action === 'new-folder') return 'dir'
    return null
  })
  const [draft, setDraft] = useState('')
  const seenReq = useRef(useUiStore.getState().fileTreeReq?.n ?? 0)

  const load = useCallback(async (rel: string) => {
    const rows = await api.listRepoDir(rel)
    if (rel === '') setRoot(rows)
    else setKids((k) => ({ ...k, [rel]: rows }))
    return rows
  }, [])

  const refresh = useCallback(async () => {
    setRefreshing(true)
    try {
      await load('')
      const open = Object.keys(kids)
      await Promise.all(open.map((rel) => load(rel)))
    } catch (e) {
      pushToast(errText(e), 'err')
    } finally {
      setRefreshing(false)
    }
  }, [kids, load, pushToast])

  useEffect(() => {
    let live = true
    api.listRepoDir('').then((rows) => { if (live) setRoot(rows) }).catch((e) => {
      if (live) pushToast(errText(e), 'err')
    })
    return () => { live = false }
  }, [pushToast])

  useEffect(() => {
    return useUiStore.subscribe((state, prev) => {
      const req = state.fileTreeReq
      if (!req || req === prev.fileTreeReq || req.n === seenReq.current) return
      seenReq.current = req.n
      if (req.action === 'refresh') void refresh()
      if (req.action === 'new-file') { setCreating('file'); setDraft('') }
      if (req.action === 'new-folder') { setCreating('dir'); setDraft('') }
    })
  }, [refresh])

  async function toggle(rel: string) {
    if (kids[rel]) {
      setKids((k) => {
        const next = { ...k }
        delete next[rel]
        return next
      })
      return
    }
    setLoadingDir(rel)
    try {
      await load(rel)
    } catch (e) {
      pushToast(errText(e), 'err')
    } finally {
      setLoadingDir((cur) => (cur === rel ? null : cur))
    }
  }

  async function commit() {
    const name = childName(draft)
    if (!name || !creating) {
      pushToast(t('tree.badName'), 'err')
      return
    }
    const path = anchor ? `${anchor}/${name}` : name
    try {
      if (creating === 'file') await api.createRepoFile(path)
      else await api.createRepoDir(path)
      setCreating(null)
      setDraft('')
      await load('')
      if (anchor) await load(anchor)
    } catch (e) {
      pushToast(errText(e), 'err')
    }
  }

  return (
    <div data-testid="file-tree" className="flex min-h-0 flex-1 flex-col px-1.5 py-2 text-[13px]">
      {/* 根行：项目名 + 新建文件 ⌥⌘N / 新建目录 ⌥⌘F / 刷新 ⌥⌘R（D14；新建目录是补设计，原型只画了新建与刷新） */}
      <div className="flex items-center gap-0.5 px-1.5 pt-0.5 pb-2">
        <span className="min-w-0 flex-1 truncate text-[12px] text-ink-3">{projectName || t('sidebar.files')}</span>
        <IconButton size="sm" label={t('tree.newFile')} action="treeNewFile" onClick={() => { setCreating('file'); setDraft('') }}>
          <Icon name="plus" size={12} />
        </IconButton>
        <IconButton size="sm" label={t('tree.newFolder')} action="treeNewFolder" onClick={() => { setCreating('dir'); setDraft('') }}>
          <Icon name="folder" size={12} />
        </IconButton>
        <IconButton size="sm" label={t('tree.refresh')} action="treeRefresh" loading={refreshing} onClick={() => void refresh()}>
          <Icon name="refresh" size={12} />
        </IconButton>
      </div>
      {creating && (
        <form onSubmit={(e) => { e.preventDefault(); void commit() }} className="flex items-center gap-1 px-1.5 pb-1.5">
          <input
            autoFocus
            aria-label={t('tree.namePh')}
            placeholder={anchor ? `${anchor}/` : t('tree.namePh')}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') { e.preventDefault(); setCreating(null) }
            }}
            className="hx-focus h-7 min-w-0 flex-1 rounded-[var(--r-xs)] bg-surface px-2 text-[12.5px] text-ink shadow-[inset_0_0_0_1px_var(--line-strong)] outline-none placeholder:text-ink-3"
          />
          <Button type="submit" variant="primary" size="xs">{t('tree.create')}</Button>
        </form>
      )}
      <div role="tree" aria-label={t('sidebar.files')} className="min-h-0 flex-1 overflow-y-auto">
        {/* A19 小号（票 13）：说明为什么空 + 一个动作 */}
        {root?.length === 0 && !creating && <Empty size="sm" icon="folder" title={t('tree.emptyDir')} desc={t('feedback.treeEmptyBody')}
          action={<Button size="sm" onClick={() => { setCreating('file'); setDraft('') }}>{t('tree.newFile')}</Button>} />}
        {root == null && <div className="flex items-center gap-2 px-2 py-3 text-[12.5px] text-ink-3"><Spinner size={12} />{t('file.loading')}</div>}
        {root && (
          <TreeList
            entries={root}
            depth={0}
            kids={kids}
            loading={loadingDir}
            current={currentPath}
            onDir={(entry) => { setAnchor(entry.path); void toggle(entry.path) }}
            onFile={(entry) => {
              setAnchor(parentOf(entry.path))
              openTab({ id: `file:${entry.path}`, kind: 'file', title: entry.name, path: entry.path })
            }}
          />
        )}
      </div>
    </div>
  )
}

/**
 * 文件树行右键菜单（A16b 第一批，owner-followups-1015 #07）：打开 · 插入到输入条 · 复制路径 · 在访达中显示 · 打开方式 ▸（#31）。
 * #32 起菜单本体在 `FileMenu`（所有文件表面同序复用）。种类不适用不渲染：目录没有「打开」（展开 = 点行本身）；
 * 链接不打开、不插入（没有一条仓内路径可指），只留复制 / 揭示——揭示指向仓外的链接由 core 拒（errors.reveal_outside_project）。
 * 右键不改选中、不打开：Trigger 只给行画焦点环（「右键一个没打开的文件时画布不跳走」）。
 */
function TreeRowMenu({ entry, onOpen, children }: { entry: RepoEntry; onOpen: () => void; children: ReactNode }) {
  const kind = entry.kind === 'dir' || entry.kind === 'link' ? entry.kind : 'file'
  return (
    <FileContextMenu path={entry.path} kind={kind} onOpen={kind === 'file' ? onOpen : undefined} insert={treeInsertPath(entry)}>
      {children}
    </FileContextMenu>
  )
}

/** 行（D14 `tn`）：28 高、圆角 6、13px ink-2；当前文件 --accent-tint 底 --ink；悬停 --hover。 */
function TreeList({ entries, depth, kids, loading, current, onDir, onFile }: {
  entries: RepoEntry[]
  depth: number
  kids: Record<string, RepoEntry[]>
  loading: string | null
  current: string | null
  onDir: (e: RepoEntry) => void
  onFile: (e: RepoEntry) => void
}) {
  return entries.map((e) => {
    const open = e.kind === 'dir' && kids[e.path] != null
    // 票 12：文件和目录可拖进输入框。链接不拖——拖了也没有一条仓内路径可指。
    // copy 不是 move：拖走的是路径文本，树上的条目和磁盘文件都留在原地。
    const drag = treeDragPayload(e)
    const on = e.kind === 'file' && current === e.path
    return (
      <div key={e.path} role="none">
        <TreeRowMenu entry={e} onOpen={() => onFile(e)}>
        <Row
          role="treeitem"
          {...{ [TREE_PATH_ATTR]: e.path }}
          data-tree-kind={e.kind}
          draggable={drag != null}
          onDragStart={(ev) => {
            if (!drag || !ev.dataTransfer) return
            ev.dataTransfer.setData(TREE_DRAG_MIME, drag.payload)
            ev.dataTransfer.setData('text/plain', drag.token)
            ev.dataTransfer.effectAllowed = 'copy'
          }}
          selected={on}
          // A16b：macOS ctrl+点击由系统当右键（发 contextmenu 开菜单），不能顺带展开 / 打开（「右键不改选中」）。
          onClick={(ev) => {
            if (isMac && ev.ctrlKey) return
            if (e.kind === 'dir') onDir(e)
            else if (e.kind === 'file') onFile(e)
          }}
          className={cn(
            'hx-focus flex h-7 items-center gap-1.5 rounded-[var(--r-xs)] pr-1.5 whitespace-nowrap',
            on ? 'bg-accent-tint text-ink' : 'text-ink-2 live:hover:bg-hover',
            e.kind === 'link' ? 'cursor-default opacity-55' : 'cursor-pointer',
          )}
          style={{ paddingLeft: 6 + depth * 8 + (e.kind === 'dir' ? 0 : 14) }}
        >
          {e.kind === 'dir' && (loading === e.path
            ? <Spinner size={12} />
            : <Icon name={open ? 'chevron-down' : 'chevron-right'} size={12} className="flex-none text-ink-3" />)}
          {e.kind === 'link'
            // 主题资产无链接形，回退描边体系
            ? <span data-icon="link" data-kind={e.kind} className="inline-flex"><Icon name="link" size={14} /></span>
            : <FileTypeIcon name={e.name} kind={e.kind} open={open} size={14} />}
          <span className="min-w-0 truncate">{e.name}</span>
        </Row>
        </TreeRowMenu>
        {open && (
          <TreeList
            entries={kids[e.path]}
            depth={depth + 1}
            kids={kids}
            loading={loading}
            current={current}
            onDir={onDir}
            onFile={onFile}
          />
        )}
      </div>
    )
  })
}
