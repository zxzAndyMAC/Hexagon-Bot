import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { RepoFileSnapshot } from '../gen/RepoFileSnapshot'
import { api, errText } from '../api'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'
import { diffLines } from '../diff'
import { FileTypeIcon } from './FileTypeIcon'
import { Md } from './Md'
import { DiffView } from './DiffView'

/// 中栏文件页（票 11 起；预览/对比回归修复见 glossary「内嵌编辑器」）。
/// 三态视图：编辑（Monaco/textarea）、预览（仅 md，实时读编辑缓冲区
/// buf——保存前就可见渲染结果）、对比（当前缓冲区 vs 基线：已保存 /
/// git HEAD / 产物版本链；版本链只对落在 `.hexagon/<path>` 上的文件出现，
/// 因为产物登记的 path 是 .hexagon 相对路径）。
/// 预览与对比用 display:none 藏编辑器而不卸载——保住撤销栈与滚动位。
/// 测试环境不挂 Monaco（happy-dom 没有 worker）；正文用 textarea，
/// 生产路径动态加载 monacoFile。两路都写同一个 textRef/buf，保存口径一致。
/// 项目身份与路径共同作 key：同一路径不能沿用上一项目的缓冲区。
export function FileEditor({ path }: { path: string }) {
  const projectRoot = useUiStore(s => s.projectRoot)
  const projectEpoch = useUiStore(s => s.projectEpoch)
  return projectRoot == null ? null : <FileBody key={`${projectEpoch}:${projectRoot}:${path}`} path={path} projectRoot={projectRoot} projectEpoch={projectEpoch} />
}

type Mode = 'edit' | 'preview' | 'compare'
/// 对比基线：'saved'=盘上现读、'head'=git HEAD、数字=产物版本号。
type Base = 'saved' | 'head' | number

function FileBody({ path, projectRoot, projectEpoch }: { path: string; projectRoot: string; projectEpoch: number }) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const saveReq = useUiStore((s) => s.saveFileReq)
  const activeTab = useUiStore((s) => s.activeTab)
  const artifacts = useUiStore((s) => s.artifacts)
  const hostRef = useRef<HTMLDivElement>(null)
  const textRef = useRef('')
  const snapshotRef = useRef<RepoFileSnapshot | null>(null)
  const saving = useRef(false)
  const liveRef = useRef(true)
  const currentProject = useCallback(() => liveRef.current && useUiStore.getState().projectRoot === projectRoot && useUiStore.getState().projectEpoch === projectEpoch, [projectRoot, projectEpoch])
  useEffect(() => { liveRef.current = true; return () => { liveRef.current = false } }, [])
  const seenSave = useRef(saveReq)
  const saveRef = useRef<() => Promise<boolean>>(async () => false)
  const editId = useId()
  const setFileEdit = useUiStore(s => s.setFileEdit)
  const [text, setText] = useState<string | null>(null)
  const [buf, setBuf] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [dirty, setDirty] = useState(false)
  const [busy, setBusy] = useState(false)
  const [mode, setMode] = useState<Mode>('edit')
  const [base, setBase] = useState<Base>('saved')
  // undefined=未探测；null=无 HEAD 基线；string=基线文本
  const [headText, setHeadText] = useState<string | null | undefined>(undefined)
  // 基线结果带 key：渲染期推导 loading，不在 effect 里同步 setState
  // （oxlint react/set-state-in-effect）。epoch 在进对比/保存后 +1 强制重拉。
  const [loaded, setLoaded] = useState<{ key: string; text: string | null }>({ key: '', text: null })
  const [basesEpoch, setBasesEpoch] = useState(0)
  const curBaseKey = `${typeof base === 'number' ? `v${base}` : base}|${basesEpoch}`
  const testing = import.meta.env.MODE === 'test'
  const isMd = /\.(md|markdown)$/i.test(path)

  useEffect(() => {
    setFileEdit(editId, { path, dirty, save: () => saveRef.current() })
    return () => setFileEdit(editId, null)
  }, [editId, path, dirty, setFileEdit])

  // `.hexagon/specs/x.md` ↔ 产物 path `specs/x.md`：文件页直接给出版本链基线。
  const artPath = path.startsWith('.hexagon/') ? path.slice('.hexagon/'.length) : null
  const versions = useMemo(
    () =>
      artPath
        ? [...new Set(artifacts.filter((a) => a.path === artPath).map((a) => a.version))].sort((a, b) => a - b)
        : [],
    [artifacts, artPath],
  )

  useEffect(() => {
    let live = true
    api.readRepoFileSnapshot(path, projectRoot).then((snapshot) => {
      if (!live || !currentProject()) return
      snapshotRef.current = snapshot
      const body = snapshot.content
      textRef.current = body
      setText(body)
      setBuf(body)
    }).catch((e) => {
      if (live && currentProject()) setErr(errText(e))
    })
    return () => { live = false }
  }, [path, projectRoot, projectEpoch, currentProject])

  useEffect(() => {
    if (testing || text == null) return
    const el = hostRef.current
    if (!el) return
    let dead = false
    let handle: { dispose: () => void } | undefined
    void import('./monacoFile').then(({ mountFileEditor }) => {
      if (dead || !hostRef.current) return
      handle = mountFileEditor(hostRef.current, path, text, (next) => {
        textRef.current = next
        // 缓冲区同时进 state：预览/对比是 React 渲染面，ref 不触发重绘。
        setBuf(next)
        setDirty(true)
      })
    }).catch((e) => setErr(errText(e)))
    return () => {
      dead = true
      handle?.dispose()
    }
  }, [path, text, testing])

  async function save() {
    const snapshot = snapshotRef.current
    if (!snapshot || saving.current || !currentProject()) return false
    saving.current = true
    setBusy(true)
    try {
      const saved = textRef.current
      await api.writeRepoFile(snapshot, saved)
      if (!currentProject()) return false
      snapshotRef.current = { ...snapshot, content: saved }
      // Editing during a pending save must not clear the newer unsaved text.
      const stillDirty = textRef.current !== saved
      setDirty(stillDirty)
      setFileEdit(editId, { path, dirty: stillDirty, save: () => saveRef.current() })
      // 保存会改变「已保存」基线；epoch +1 让开着的对比重拉，免得 diff 说反话。
      setBasesEpoch((e) => e + 1)
      pushToast(t('file.saved'), 'ok')
      return !stillDirty
    } catch (e) {
      if (currentProject()) pushToast(errText(e), 'err')
      return false
    } finally {
      saving.current = false
      if (currentProject()) setBusy(false)
    }
  }

  useEffect(() => {
    saveRef.current = save
  })

  useEffect(() => {
    if (saveReq === seenSave.current) return
    seenSave.current = saveReq
    if (activeTab !== `file:${path}`) return
    void saveRef.current()
  }, [saveReq, activeTab, path])

  // 进对比才探测 HEAD：非 git 仓/未跟踪文件回 null，HEAD 选项直接不出现。
  useEffect(() => {
    if (mode !== 'compare' || headText !== undefined) return
    let live = true
    api.repoFileHead(path)
      .then((body) => { if (live) setHeadText(body) })
      .catch(() => { if (live) setHeadText(null) })
    return () => { live = false }
  }, [mode, headText, path])

  // 拉当前选中基线的文本。「已保存」每次现读盘上内容——打开文件页期间
  // agent 可能又写过同一文件，缓存的旧读数会把 diff 做反。
  useEffect(() => {
    if (mode !== 'compare') return
    // HEAD 还在探测时先不拉——免得闪一帧「基线不可用」再变回 diff。
    if (base === 'head' && headText === undefined) return
    let live = true
    const key = curBaseKey
    const load = async () => {
      let body: string | null
      try {
        if (base === 'saved') body = await api.readRepoFile(path)
        else if (base === 'head') body = headText ?? null
        else body = artPath ? await api.artifactContentAt(artPath, base) : null
      } catch {
        body = null
      }
      if (live) setLoaded({ key, text: body })
    }
    void load()
    return () => { live = false }
  }, [mode, base, curBaseKey, path, artPath, headText])
  const baseLoading = mode === 'compare' && loaded.key !== curBaseKey
  const baseText = loaded.key === curBaseKey ? loaded.text : null

  const ops = useMemo(
    () => (baseText == null ? [] : diffLines(baseText.split('\n'), buf.split('\n'))),
    [baseText, buf],
  )
  const changed = ops.some((o) => o.type !== 'eq')

  const saveTip = `${t('file.save')} ${formatBinding(bindingFor('saveFile'))}`

  return (
    <div data-testid="file-editor" data-path={path} style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      <div className="row-line" style={{ padding: '6px 14px', display: 'flex', gap: 8, alignItems: 'center' }}>
        <FileTypeIcon name={path.split('/').pop() ?? path} kind="file" size={13} style={{ flexShrink: 0 }} />
        <span className="mono" title={path} style={{ fontWeight: 560, fontSize: 12.5, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{path}</span>
        {dirty && <span className="dot warn" title={t('file.unsaved')} style={{ flexShrink: 0 }} />}
        <div style={{ flex: 1 }} />
        {text != null && (
          <div className="seg" role="group" aria-label={t('file.mode')}>
            <button type="button" className={mode === 'edit' ? 'on' : ''} onClick={() => setMode('edit')}>{t('file.edit')}</button>
            {isMd && <button type="button" className={mode === 'preview' ? 'on' : ''} onClick={() => setMode('preview')}>{t('file.preview')}</button>}
            <button type="button" className={mode === 'compare' ? 'on' : ''} onClick={() => { setMode('compare'); setBasesEpoch((e) => e + 1) }}>{t('file.compare')}</button>
          </div>
        )}
        <button
          className="btn primary"
          type="button"
          title={saveTip}
          disabled={text == null || !dirty || busy}
          onClick={() => void save()}
          style={{ fontSize: 11, padding: '2px 8px' }}
        >
          {busy ? t('file.saving') : t('file.save')}
        </button>
      </div>
      {text != null && mode === 'compare' && (
        <div className="row-line" style={{ padding: '5px 14px', display: 'flex', gap: 8, alignItems: 'center' }}>
          <span className="dim3" style={{ fontSize: 11 }}>{t('file.base')}</span>
          <div className="seg" role="group" aria-label={t('file.base')}>
            <button type="button" className={base === 'saved' ? 'on' : ''} onClick={() => setBase('saved')}>{t('file.baseSaved')}</button>
            {headText != null && <button type="button" className={base === 'head' ? 'on' : ''} onClick={() => setBase('head')}>HEAD</button>}
            {versions.map((v) => (
              <button key={v} type="button" className={base === v ? 'on' : ''} onClick={() => setBase(v)}>{t('file.version', { n: v })}</button>
            ))}
          </div>
          <span className="dim3" style={{ fontSize: 11 }}>→ {t('file.current')}</span>
        </div>
      )}
      {err && <div className="dim3" style={{ padding: 14, fontSize: 12 }}>{err}</div>}
      {text == null && !err && <div className="dim3" style={{ padding: 14, fontSize: 12 }}>{t('file.loading')}</div>}
      {text != null && (
        <div style={{ flex: 1, minHeight: 0, display: mode === 'edit' ? 'flex' : 'none', flexDirection: 'column' }}>
          {testing ? (
            <textarea
              aria-label={path}
              value={buf}
              onChange={(e) => {
                textRef.current = e.target.value
                setBuf(e.target.value)
                setDirty(true)
              }}
              style={{ flex: 1, minHeight: 0, resize: 'none', border: 'none', background: 'transparent', color: 'inherit', fontFamily: 'inherit', padding: 14 }}
            />
          ) : (
            <div ref={hostRef} style={{ flex: 1, minHeight: 0 }} />
          )}
        </div>
      )}
      {text != null && mode === 'preview' && (
        <div className="msg-body" style={{ flex: 1, minHeight: 0, overflowY: 'auto', maxWidth: 'none', margin: '6px 14px 14px' }}>
          <Md>{buf}</Md>
        </div>
      )}
      {text != null && mode === 'compare' && (
        baseLoading ? (
          <div className="dim3" style={{ flex: 1, minHeight: 0, padding: 14, fontSize: 12 }}>…</div>
        ) : baseText == null ? (
          <div className="dim3" style={{ flex: 1, minHeight: 0, padding: 14, fontSize: 12 }}>{t('file.noBase')}</div>
        ) : !changed ? (
          <div className="dim3" style={{ flex: 1, minHeight: 0, padding: 14, fontSize: 12 }}>{t('file.noChanges')}</div>
        ) : (
          <DiffView ops={ops} />
        )
      )}
    </div>
  )
}
