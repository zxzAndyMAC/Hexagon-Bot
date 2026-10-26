// 文件编辑器 FileEditor（components D14，canvas#file；interaction §6「文件」）。ui-redesign 票 09 换皮。
// 子头：面包屑「项目 › 路径」+ warn 点「未保存」+ A7 编辑 / 预览 / 对比 + 保存 ⌘S（primary sm）；保存失败子头红字、保留缓冲。
// 三态视图：编辑（Monaco/textarea）、预览（仅 md，实时读编辑缓冲区 buf——保存前就可见渲染结果；非 md「预览」禁用写原因）、
// 对比（当前缓冲区 vs 基线：已保存 / git HEAD / 产物版本链；版本链只对落在 `.hexagon/<path>` 上的文件出现，
// 因为产物登记的 path 是 .hexagon 相对路径）。
// 预览与对比用 display:none 藏编辑器而不卸载——保住撤销栈与滚动位（票 09 验收：切换不卸载编辑器）。
// 测试环境不挂 Monaco（happy-dom 没有 worker）；正文用 textarea，生产路径动态加载 monacoFile。
// 两路都写同一个 textRef/buf，保存口径一致。项目身份与路径共同作 key：同一路径不能沿用上一项目的缓冲区。
// 选中文字出选区浮条（D14 selbar）：编辑态取 Monaco / textarea 选区，预览态取 DOM 选区。
import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { RepoFileSnapshot } from '../gen/RepoFileSnapshot'
import { api, errText } from '../api'
import { bindingFor } from '../keymap'
import { useUiStore } from '../store'
import { diffLines } from '../diff'
import { langFor } from '../highlight'
import { Md } from './Md'
import { Icon } from './Icon'
import { DiffCode } from './canvas/CodeView'
import { SelectionBar } from './canvas/SelectionBar'
import { textareaSelection, useDomSelection, type SelectionAnchor } from './canvas/useSelection'
import { Button } from '@/components/ui/button'
import { Dot } from '@/components/ui/dot'
import { ShortcutKbd } from '@/components/ui/kbd'
import { Segmented, SegmentedItem } from '@/components/ui/toggle-group'
import { ProjectFileMenu } from './FileMenu'

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
  const projectName = useUiStore((s) => s.projectName)
  const hostRef = useRef<HTMLDivElement>(null)
  const bodyRef = useRef<HTMLDivElement>(null)
  const previewRef = useRef<HTMLDivElement>(null)
  const textRef = useRef('')
  const snapshotRef = useRef<RepoFileSnapshot | null>(null)
  const saving = useRef(false)
  const liveRef = useRef(true)
  const monacoRef = useRef<{ setValue: (next: string) => void } | null>(null)
  const currentProject = useCallback(() => liveRef.current && useUiStore.getState().projectRoot === projectRoot && useUiStore.getState().projectEpoch === projectEpoch, [projectRoot, projectEpoch])
  useEffect(() => { liveRef.current = true; return () => { liveRef.current = false } }, [])
  const seenSave = useRef(saveReq)
  const saveRef = useRef<() => Promise<boolean>>(async () => false)
  const discardRef = useRef<() => void>(() => {})
  const editId = useId()
  const setFileEdit = useUiStore(s => s.setFileEdit)
  const [text, setText] = useState<string | null>(null)
  const [buf, setBuf] = useState('')
  const [err, setErr] = useState<string | null>(null)
  const [saveErr, setSaveErr] = useState<string | null>(null)
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
  const [anchor, setAnchor] = useState<SelectionAnchor | null>(null)
  const closeBar = useCallback(() => setAnchor(null), [])
  const pointer = useRef<{ x: number; y: number } | undefined>(undefined)
  // textarea 选区用原生 select 事件（React 的 onSelect 走 selectionchange 合成，元素没聚焦时不触发）
  const taRef = useRef<HTMLTextAreaElement>(null)
  const hasText = text != null
  useEffect(() => {
    const ta = taRef.current
    if (!ta) return
    const onSelect = () => setAnchor(bodyRef.current ? textareaSelection(ta, path, bodyRef.current, pointer.current) : null)
    ta.addEventListener('select', onSelect)
    return () => ta.removeEventListener('select', onSelect)
  }, [path, hasText])

  useEffect(() => {
    setFileEdit(editId, { path, dirty, save: () => saveRef.current(), discard: () => discardRef.current() })
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
    let handle: { dispose: () => void; setValue: (next: string) => void } | undefined
    void import('./monacoFile').then(({ mountFileEditor }) => {
      if (dead || !hostRef.current) return
      handle = mountFileEditor(hostRef.current, path, text, (next) => {
        textRef.current = next
        // 缓冲区同时进 state：预览/对比是 React 渲染面，ref 不触发重绘。
        setBuf(next)
        setDirty(true)
      }, (sel) => {
        const host = hostRef.current
        const body = bodyRef.current
        if (!sel || !host || !body) { setAnchor(null); return }
        const dx = host.getBoundingClientRect().left - body.getBoundingClientRect().left
        const dy = host.getBoundingClientRect().top - body.getBoundingClientRect().top
        setAnchor({ sel: { path, text: sel.text, startLine: sel.startLine, endLine: sel.endLine }, x: sel.x + dx, y: sel.y + dy, bottom: sel.bottom + dy })
      })
      monacoRef.current = handle
    }).catch((e) => setErr(errText(e)))
    return () => {
      dead = true
      monacoRef.current = null
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
      setSaveErr(null)
      setFileEdit(editId, { path, dirty: stillDirty, save: () => saveRef.current(), discard: () => discardRef.current() })
      // 保存会改变「已保存」基线；epoch +1 让开着的对比重拉，免得 diff 说反话。
      setBasesEpoch((e) => e + 1)
      pushToast(t('file.saved'), 'ok')
      return !stillDirty
    } catch (e) {
      // D14 错误行：保存失败子头红字，缓冲保留
      if (currentProject()) { setSaveErr(errText(e)); pushToast(errText(e), 'err') }
      return false
    } finally {
      saving.current = false
      if (currentProject()) setBusy(false)
    }
  }

  /** 「放弃修改」：回到最后一次已知的盘上文本（D17）。 */
  function discard() {
    const saved = snapshotRef.current?.content
    if (saved == null) return
    textRef.current = saved
    setBuf(saved)
    monacoRef.current?.setValue(saved)
    setDirty(false)
    setSaveErr(null)
  }

  useEffect(() => {
    saveRef.current = save
    discardRef.current = discard
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

  useDomSelection(previewRef, path, setAnchor, mode === 'preview' && text != null)
  const switchMode = (next: Mode) => {
    setAnchor(null)
    setMode(next)
    if (next === 'compare') setBasesEpoch((e) => e + 1)
  }

  const crumbs = path.split('/')
  const note = (s: string) => <div className="p-3.5 text-[12.5px] text-ink-3">{s}</div>

  return (
    <div data-testid="file-editor" data-path={path} className="flex min-h-0 min-w-0 flex-1 flex-col">
      {/* 子头（cv-sub） */}
      <div className="flex min-h-12 flex-wrap items-center gap-2 border-b border-line-soft px-3.5 py-2.5">
        {/* #32：面包屑右键 = 文件菜单（此页即已打开，无「打开」） */}
        <ProjectFileMenu path={path}>
        <nav aria-label={path} title={path} className="flex min-w-0 items-center gap-1 text-[13px] text-ink-3">
          {projectName && <span className="truncate">{projectName}</span>}
          {crumbs.map((c, i) => (
            <span key={i} className="flex min-w-0 items-center gap-1">
              {(i > 0 || projectName) && <Icon name="chevron-right" size={12} className="flex-none" />}
              <span className={i === crumbs.length - 1 ? 'truncate text-ink' : 'truncate'}>{c}</span>
            </span>
          ))}
          {dirty && (
            <span data-unsaved className="ml-1 flex flex-none items-center gap-1 text-[12px]">
              <Dot tone="warn" />{t('file.unsaved')}
            </span>
          )}
        </nav>
        </ProjectFileMenu>
        {saveErr && <span role="alert" className="min-w-0 truncate text-[12px] text-red-ink" title={saveErr}>{t('file.saveFailed')}</span>}
        <span className="flex-1" />
        {text != null && (
          <Segmented value={mode} onValueChange={(v) => switchMode(v as Mode)} aria-label={t('file.mode')}>
            <SegmentedItem value="edit">{t('file.edit')}</SegmentedItem>
            <SegmentedItem value="preview" disabled={!isMd} disabledReason={t('file.previewMdOnly')}>{t('file.preview')}</SegmentedItem>
            <SegmentedItem value="compare">{t('file.compare')}</SegmentedItem>
          </Segmented>
        )}
        <Button variant="primary" size="sm" title={t('file.save')} data-file-save
          disabled={text == null || !dirty} loading={busy} onClick={() => void save()}>
          {busy ? t('file.saving') : t('file.save')}
          <ShortcutKbd binding={bindingFor('saveFile')} />
        </Button>
      </div>
      {text != null && mode === 'compare' && (
        <div className="flex flex-wrap items-center gap-2 border-b border-line-soft px-3.5 py-2">
          <span className="text-[12px] text-ink-3">{t('file.base')}</span>
          <Segmented value={String(base)} onValueChange={(v) => setBase(v === 'saved' || v === 'head' ? v : Number(v))} aria-label={t('file.base')}>
            <SegmentedItem value="saved">{t('file.baseSaved')}</SegmentedItem>
            {headText != null && <SegmentedItem value="head">HEAD</SegmentedItem>}
            {versions.map((v) => <SegmentedItem key={v} value={String(v)}>{t('file.version', { n: v })}</SegmentedItem>)}
          </Segmented>
          <span className="text-[12px] text-ink-3">→ {t('file.current')}</span>
        </div>
      )}
      <div ref={bodyRef} className="relative flex min-h-0 flex-1 flex-col">
        {err && note(err)}
        {text == null && !err && note(t('file.loading'))}
        {text != null && (
          <div className="min-h-0 flex-1 flex-col" style={{ display: mode === 'edit' ? 'flex' : 'none' }}>
            {testing ? (
              <textarea
                aria-label={path}
                value={buf}
                onChange={(e) => {
                  textRef.current = e.target.value
                  setBuf(e.target.value)
                  setDirty(true)
                }}
                ref={taRef}
                onMouseUp={(e) => { pointer.current = { x: e.clientX, y: e.clientY } }}
                className="min-h-0 flex-1 resize-none bg-transparent p-3.5 font-mono text-[12px] leading-[1.75] text-ink outline-none"
              />
            ) : (
              <div ref={hostRef} className="min-h-0 flex-1" />
            )}
          </div>
        )}
        {text != null && mode === 'preview' && (
          <div ref={previewRef} data-file-preview className="msg-body md-doc min-h-0 flex-1 overflow-y-auto">
            <Md>{buf}</Md>
          </div>
        )}
        {text != null && mode === 'compare' && (
          baseLoading ? note('…')
            : baseText == null ? note(t('file.noBase'))
            : !changed ? note(t('file.noChanges'))
            : <DiffCode ops={ops} before={baseText} after={buf} lang={langFor(path)} label={path} />
        )}
        <SelectionBar anchor={anchor} onClose={closeBar} />
      </div>
    </div>
  )
}
