import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText, type RepoEntry } from '../api'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'
import { fileIcon } from './fileIcon'
import { Icon } from './Icon'
import { Row } from './Row'

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
    try {
      await load('')
      const open = Object.keys(kids)
      await Promise.all(open.map((rel) => load(rel)))
    } catch (e) {
      pushToast(errText(e), 'err')
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
    try {
      await load(rel)
    } catch (e) {
      pushToast(errText(e), 'err')
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

  const tip = (action: 'treeNewFile' | 'treeNewFolder' | 'treeRefresh', label: string) =>
    `${label} ${formatBinding(bindingFor(action))}`

  return (
    <div data-testid="file-tree" style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      <div style={{ display: 'flex', gap: 4, padding: '4px 8px', alignItems: 'center' }}>
        <button
          className="btn"
          type="button"
          title={tip('treeNewFile', t('tree.newFile'))}
          style={{ fontSize: 11, padding: '2px 6px', display: 'inline-flex', alignItems: 'center', gap: 4 }}
          onClick={() => { setCreating('file'); setDraft('') }}
        >
          <Icon name="plus" size={11} /> {t('tree.newFile')}
        </button>
        <button
          className="btn"
          type="button"
          title={tip('treeNewFolder', t('tree.newFolder'))}
          style={{ fontSize: 11, padding: '2px 6px', display: 'inline-flex', alignItems: 'center', gap: 4 }}
          onClick={() => { setCreating('dir'); setDraft('') }}
        >
          <Icon name="folder" size={11} /> {t('tree.newFolder')}
        </button>
        <button
          className="icon-btn"
          type="button"
          title={tip('treeRefresh', t('tree.refresh'))}
          style={{ marginLeft: 'auto', display: 'inline-flex', alignItems: 'center' }}
          onClick={() => void refresh()}
        >
          <Icon name="refresh" size={12} />
        </button>
      </div>
      {creating && (
        <form
          onSubmit={(e) => { e.preventDefault(); void commit() }}
          style={{ display: 'flex', gap: 4, padding: '0 8px 6px', alignItems: 'center' }}
        >
          <input
            className="input"
            autoFocus
            aria-label={t('tree.namePh')}
            placeholder={anchor ? `${anchor}/` : t('tree.namePh')}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') { e.preventDefault(); setCreating(null) }
            }}
            style={{ flex: 1, width: 'auto', fontSize: 12, padding: '3px 6px' }}
          />
          <button className="btn primary" type="submit" style={{ fontSize: 11, padding: '2px 6px' }}>
            {t('tree.create')}
          </button>
        </form>
      )}
      <div role="tree" style={{ flex: 1, overflowY: 'auto' }}>
        {root?.length === 0 && <div className="dim3" style={{ padding: '14px 12px', fontSize: 11 }}>{t('tree.emptyDir')}</div>}
        {root == null && <div className="dim3" style={{ padding: '14px 12px', fontSize: 11 }}>{t('file.loading')}</div>}
        {root && (
          <TreeList
            entries={root}
            depth={0}
            kids={kids}
            anchor={anchor}
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

function TreeList({ entries, depth, kids, anchor, onDir, onFile }: {
  entries: RepoEntry[]
  depth: number
  kids: Record<string, RepoEntry[]>
  anchor: string
  onDir: (e: RepoEntry) => void
  onFile: (e: RepoEntry) => void
}) {
  return entries.map((e) => {
    const icon = fileIcon(e.name, e.kind)
    const open = e.kind === 'dir' && kids[e.path] != null
    return (
      <div key={e.path}>
        <Row
          role="treeitem"
          selected={anchor === e.path}
          onClick={() => (e.kind === 'dir' ? onDir(e) : e.kind === 'file' ? onFile(e) : undefined)}
          style={{
            padding: '3px 8px',
            paddingLeft: 8 + depth * 12,
            display: 'flex',
            gap: 6,
            alignItems: 'center',
            cursor: e.kind === 'link' ? 'default' : 'pointer',
            opacity: e.kind === 'link' ? 0.55 : 1,
          }}
        >
          {e.kind === 'dir'
            ? <Icon name={open ? 'chevron-down' : 'chevron-right'} size={11} />
            : <span style={{ width: 11, flexShrink: 0 }} />}
          <span data-icon={icon} data-kind={e.kind} style={{ display: 'inline-flex' }}>
            <Icon name={icon} size={13} />
          </span>
          <span className="mono" style={{ fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{e.name}</span>
        </Row>
        {open && kids[e.path].length === 0 && (
          <div className="dim3" style={{ paddingLeft: 28 + depth * 12, fontSize: 11 }} />
        )}
        {open && (
          <TreeList
            entries={kids[e.path]}
            depth={depth + 1}
            kids={kids}
            anchor={anchor}
            onDir={onDir}
            onFile={onFile}
          />
        )}
      </div>
    )
  })
}
