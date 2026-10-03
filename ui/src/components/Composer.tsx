import { AgentScreen } from './AgentScreen'
import { ElementDraft } from './ElementReferences'
import type { ElementRef } from '../gen/ElementRef'
import { ProjectApprovalMode } from './ProjectApprovalMode'
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api, errText } from '../api'
import { Icon } from './Icon'
import { ContextMeter } from './ContextMeter'
import type { AttachRef } from '../gen/AttachRef'
import {
  atomicDeletion,
  findAtoms,
  insertPathToken,
  mentionTokenText,
  pathFromTreeDrag,
  pathTokenText,
  TREE_DRAG_MIME,
  type Atom,
} from '../composerAtoms'

// 票 03：图片附件——粘贴/拖拽进 Composer → stage_attachment 落
// .hexagon/inbox/ → chip 条可移除 → send/dispatch 带引用。
// 前端拦截口径与核内一致：单图 ≤5MB、单条 ≤4 图、只收图片。
const ATTACH_IMG_CAP = 5 * 1024 * 1024
const ATTACH_MAX = 4
const IMG_EXT: Record<string, string> = {
  'image/png': 'png', 'image/jpeg': 'jpg', 'image/gif': 'gif', 'image/webp': 'webp',
}

// ui-audit-2 票 09：`#` 路径补全走核 API（repo_paths IPC，仓根有界
// 遍历）——MOCK_PATHS 占位数据已退役。防抖 120ms + seq 乱序守卫。

// 发送历史（ui-audit 票 10 / P2-10）：session 内存不落盘——
// 空输入按 ↑ 回填上一条，继续 ↑ 向前翻、↓ 向后翻。
const HISTORY: string[] = []
const HISTORY_CAP = 50

type Staged = AttachRef & { preview: string }

function ComposerMirror({ text, atoms }: { text: string; atoms: Atom[] }) {
  if (!text) return null
  const parts: ReactNode[] = []
  let i = 0
  for (const a of atoms) {
    if (a.start > i) parts.push(<span key={`t${i}`}>{text.slice(i, a.start)}</span>)
    parts.push(
      <span key={`a${a.start}`} className="atom-token" data-kind={a.kind} data-value={a.value}>
        {text.slice(a.start, a.end)}
      </span>,
    )
    i = a.end
  }
  if (i < text.length) parts.push(<span key={`t${i}`}>{text.slice(i)}</span>)
  // textarea 在文末换行时多出一行；div 的 pre-wrap 会吃掉结尾 \n，补一个 br 对齐。
  return <>{parts}{text.endsWith('\n') ? <br /> : null}</>
}

function dropKind(dt: DataTransfer | null): 'path' | 'files' | null {
  if (!dt) return null
  const types = Array.from(dt.types)
  // 路径优先于 Files：树节点不带文件字节。外部文件拖入仍走图片附件。
  if (types.includes(TREE_DRAG_MIME)) return 'path'
  if (types.includes('Files')) return 'files'
  return null
}

function fit(ta: HTMLTextAreaElement) {
  ta.style.height = 'auto'
  ta.style.height = `${Math.min(ta.scrollHeight, 140)}px`
}

export function Composer() {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const invalidate = useUiStore((s) => s.invalidate)
  const pushToast = useUiStore((s) => s.pushToast)
  const mcpPending = useUiStore((s) => s.mcpPending)
  const [submitting, setSubmitting] = useState(false)
  const submittingRef = useRef(false)
  const [text, setText] = useState('')
  const [popup, setPopup] = useState<{ kind: '@' | '#'; items: { label: string; hint: string }[] } | null>(null)
  const [sel, setSel] = useState(0)
  const popupId = useId()
  const suggestions = useRef<HTMLDivElement>(null)
  useEffect(() => { suggestions.current?.querySelector('[aria-selected="true"]')?.scrollIntoView?.({ block: 'nearest' }) }, [popup, sel])
  const [histIdx, setHistIdx] = useState(-1)
  const [attachments, setAttachments] = useState<Staged[]>([])
  const [elements, setElements] = useState<ElementRef[]>([])
  const [zoom, setZoom] = useState<Staged | null>(null)
  const [dragging, setDragging] = useState(false)
  const inputRef = useRef<HTMLTextAreaElement>(null)
  const mirrorRef = useRef<HTMLDivElement>(null)
  const zoomCloseRef = useRef<HTMLButtonElement>(null)
  const zoomWas = useRef(false)
  const pendingCaret = useRef<number | null>(null)
  const pathSeq = useRef(0)
  const pathTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  // reliability 07: duplicate roles need stable instance tokens; a role label
  // alone previously sent both menu entries to the first agent.
  const mentionName = (m: typeof team[number]) => team.filter((peer) => peer.role === m.role).length > 1
    ? `${m.role}[${m.id}]` : m.role
  const roster = useMemo(() => new Set(team.flatMap((m) => [m.role, `${m.role}[${m.id}]`])), [team])
  const atoms = useMemo(() => findAtoms(text, roster), [text, roster])

  const fetchPaths = (q: string) => {
    if (pathTimer.current) clearTimeout(pathTimer.current)
    const seq = ++pathSeq.current
    pathTimer.current = setTimeout(() => {
      api.repoPaths(q)
        .then((ps) => {
          if (pathSeq.current !== seq) return // 晚到的旧查询不盖新结果
          setPopup({
            kind: '#',
            items: ps.map((p) => ({
              label: `#${p}`,
              hint: p.endsWith('/') ? t('composer.directory') : t('composer.file'),
            })),
          })
          setSel(0)
        })
        .catch(() => { if (pathSeq.current === seq) setPopup(null) })
    }, 120)
  }

  const syncPopup = (v: string) => {
    const last = v.slice(v.lastIndexOf(' ') + 1)
    if (last.startsWith('@')) {
      const q = last.slice(1)
      setPopup({
        kind: '@',
        items: team
          .filter((m) => m.role.includes(q) || m.id.includes(q))
          .map((m) => ({ label: mentionTokenText(mentionName(m)), hint: t('composer.mentionHint') })),
      })
      setSel(0)
    } else if (last.startsWith('#')) {
      setPopup({ kind: '#', items: [] }) // 先开空壳，结果异步到
      fetchPaths(last.slice(1))
    } else {
      setPopup(null)
    }
  }

  const onChange = (v: string) => {
    setText(v)
    setHistIdx(-1) // 手动编辑即退出历史浏览
    syncPopup(v)
  }

  // 票 10：弹层选中与退格删块都走这里，光标落在新正文的 caret 上，
  // 这样「点名之后退一次」打在块尾（含补上的收束空格），不会先吃掉一个字。
  const commitText = (v: string, caret: number) => {
    pendingCaret.current = caret
    setText(v)
    setHistIdx(-1)
    syncPopup(v)
  }

  const pick = (label: string) => {
    const head = text.slice(0, text.lastIndexOf(' ') + 1)
    // # 与 @ 都收成同一套 token 文本。pathTokenText 只保留这一条路径——
    // 目录（尾 /）不展开子文件；票 12 拖入必须复用它，不能另写一套。
    const token = label.startsWith('#')
      ? pathTokenText(label)
      : label.startsWith('@')
        ? mentionTokenText(label)
        : label
    const next = `${head}${token} `
    commitText(next, next.length)
    setPopup(null)
    inputRef.current?.focus()
  }

  useEffect(() => {
    const c = pendingCaret.current
    const ta = inputRef.current
    if (c == null || !ta) return
    pendingCaret.current = null
    ta.setSelectionRange(c, c)
    fit(ta)
  }, [text])

  // 放大层关掉之后焦点回到输入框（票 10）。开着时焦点在关闭钮上，Esc 由窗口捕获。
  useEffect(() => {
    if (zoom) {
      zoomWas.current = true
      zoomCloseRef.current?.focus()
      return
    }
    if (!zoomWas.current) return
    zoomWas.current = false
    inputRef.current?.focus()
  }, [zoom])

  useEffect(() => {
    if (!zoom) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      e.stopPropagation()
      setZoom(null)
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [zoom])

  // 票 03：文件 → chip。前端先拦（类型/尺寸/数量），过了才调
  // stage_attachment 落盘——核内魔数复核仍在（双判不信任前端）。
  const addFiles = async (files: File[]) => {
    if (submittingRef.current) return
    for (const f of files) {
      if (attachments.length >= ATTACH_MAX) {
        pushToast(t('composer.attachTooMany', { max: ATTACH_MAX }), 'err')
        return
      }
      if (!IMG_EXT[f.type]) {
        pushToast(t('composer.attachNotImage', { name: f.name }), 'err')
        continue
      }
      if (f.size > ATTACH_IMG_CAP) {
        pushToast(t('composer.attachTooBig', { name: f.name }), 'err')
        continue
      }
      try {
        const bytes = new Uint8Array(await f.arrayBuffer())
        const ref = await api.stageAttachment(f.name, bytes)
        // 预览走 object URL（字节已在 inbox 落盘，不另起 IPC 读回）。
        const preview = URL.createObjectURL(new Blob([bytes], { type: ref.media_type }))
        setAttachments((a) => [...a, { ...ref, preview }])
      } catch (e) {
        pushToast(errText(e), 'err')
      }
    }
  }

  const removeAttachment = async (ref: AttachRef) => {
    if (submittingRef.current) return
    setZoom((z) => (z?.path === ref.path ? null : z))
    setAttachments((a) => a.filter((x) => x.path !== ref.path))
    await api.discardAttachments([ref]).catch(() => {})
  }

  const send = async () => {
    if (mcpPending || submittingRef.current || (!text.trim() && attachments.length === 0 && elements.length === 0)) return
    // Owner 2026-10-01: sending and stopping are different actions. The busy
    // interval ends after message persistence, not after the team's entire run.
    submittingRef.current = true
    setSubmitting(true)
    const body = text
    const refs = attachments.map(({ preview: _p, ...r }) => r)
    // ui-audit 票 04（P1-6）：发送失败 toast + 草稿保留——
    // 原先 await 裸抛，文案随输入框状态悬在用户面前却无任何反馈。
    try {
      if (elements.length) await api.sendElementMessage(body, refs, elements.map(item => item.id), elements[0].project_root)
      else await api.sendMessage(body, refs)
      // 发送成功才贴底。失败走下面的 catch，视口留在用户正在看的地方。
      useUiStore.getState().requestTimelineStick()
      // 票 09：点名、没点名的封闭选择、卸掉项目经理后的接话人，都在
      // send_message → route_unnamed_owner。界面再 dispatch 会双发，
      // 也会把 L0/L1 的角色点名闸重写成「看见 @ 就派」。
      setText('')
      if (inputRef.current) inputRef.current.style.height = 'auto'
      attachments.forEach((a) => URL.revokeObjectURL(a.preview))
      setAttachments([])
      setElements([])
      setHistIdx(-1)
      HISTORY.unshift(body)
      if (HISTORY.length > HISTORY_CAP) HISTORY.pop()
      await invalidate()
    } catch (e) {
      // 票 03：发送失败保留 chip 与已落盘文件——重发直接可用；
      // 未发送的孤儿文件由 inbox 周期清扫兜底（见 stage_attachment）。
      pushToast(errText(e), 'err')
    } finally {
      submittingRef.current = false
      setSubmitting(false)
    }
  }

  // 票 12：未聚焦时插到文末（拖进来的人多半没把光标点在半截词上）。
  // 聚焦时插在光标处，并补收束空格，使这块和手打路径用同一次退格删掉。
  const insertDroppedPath = (path: string) => {
    const ta = inputRef.current
    const caret = ta && document.activeElement === ta ? (ta.selectionStart ?? text.length) : text.length
    const next = insertPathToken(text, caret, path)
    if (!next) return
    commitText(next.text, next.caret)
    ta?.focus()
  }

  return (
    <div
      data-testid="composer"
      style={{
        position: 'relative', padding: '10px 14px', borderTop: '1px solid var(--border)',
        background: dragging ? 'var(--bg-2)' : 'var(--bg-1)',
        outline: dragging ? '1px dashed var(--accent)' : 'none',
        outlineOffset: -4,
      }}
      // 票 03：拖拽图片进 Composer（dragover 必须 preventDefault 才会触发 drop）。
      // 票 12：文件树节点走自定义 MIME，插入 pathTokenText，不读、不写文件。
      onDragOver={(e) => {
        if (!dropKind(e.dataTransfer)) return
        e.preventDefault()
        if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'
        setDragging(true)
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={(e) => {
        const kind = dropKind(e.dataTransfer)
        if (!kind) return
        e.preventDefault()
        setDragging(false)
        if (kind === 'path') {
          const path = pathFromTreeDrag(e.dataTransfer?.getData(TREE_DRAG_MIME) ?? '')
          if (path) insertDroppedPath(path)
          return
        }
        void addFiles(Array.from(e.dataTransfer?.files ?? []))
      }}
    >
      {/* 票 03 + 票 10：按图显示。关闭钮在图上，只去掉这一张；点图放大。 */}
      <ElementDraft references={elements} onChange={setElements} submitting={submitting} trailing={<AgentScreen />} />
      {attachments.length > 0 && (
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginBottom: 8 }}>
          {attachments.map((a) => (
            <div key={a.path} className="attach-chip" style={{ position: 'relative', width: 72, height: 72 }}>
              <button
                type="button"
                className="attach-thumb"
                aria-label={t('composer.imageZoom', { name: a.name })}
                onClick={() => setZoom(a)}
                style={{
                  padding: 0, border: '1px solid var(--border)', borderRadius: 8,
                  background: 'var(--bg-2)', cursor: 'pointer', width: 72, height: 72, display: 'block',
                }}
              >
                <img
                  src={a.preview}
                  alt={a.name}
                  style={{ width: '100%', height: '100%', objectFit: 'cover', borderRadius: 7, display: 'block' }}
                />
              </button>
              <button
                type="button"
                className="attach-remove"
                aria-label={t('composer.attachRemove')}
                onClick={() => void removeAttachment(a)}
                style={{
                  position: 'absolute', top: 3, right: 3, width: 18, height: 18, borderRadius: 9,
                  border: 'none', cursor: 'pointer', padding: 0,
                  background: 'var(--bg)', color: 'var(--text-2)',
                  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
                }}
              >
                <Icon name="close" size={11} />
              </button>
            </div>
          ))}
        </div>
      )}
      {zoom && (
        <div
          className="attach-zoom"
          role="dialog"
          aria-modal="true"
          aria-label={t('composer.imageZoom', { name: zoom.name })}
          onClick={() => setZoom(null)}
          style={{
            position: 'fixed', inset: 0, zIndex: 80,
            background: 'rgba(0,0,0,.55)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
          }}
        >
          <img
            src={zoom.preview}
            alt={zoom.name}
            onClick={(e) => e.stopPropagation()}
            style={{ maxWidth: '90vw', maxHeight: '86vh', borderRadius: 8, boxShadow: '0 12px 40px rgba(0,0,0,.4)' }}
          />
          <button
            type="button"
            ref={zoomCloseRef}
            className="attach-zoom-close"
            aria-label={t('composer.previewClose')}
            onClick={(e) => { e.stopPropagation(); setZoom(null) }}
            style={{
              position: 'absolute', top: 16, right: 16, width: 28, height: 28, borderRadius: 8,
              border: '1px solid var(--border)', cursor: 'pointer', padding: 0,
              background: 'var(--popover)', color: 'var(--text)',
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
            }}
          >
            <Icon name="close" size={14} />
          </button>
        </div>
      )}
      {/* Owner issue16: suggestions anchor to the input, above the reference shelf. */}
      <div className="composer-box" style={{ position: 'relative' }}>
      {popup && popup.items.length > 0 && (
        <div ref={suggestions} id={popupId} className="panel panel-float composer-suggestions" role="listbox" aria-label={popup.kind === '@' ? t('composer.mentionHint') : t('composer.pathHint')} style={{ position: 'absolute', bottom: '100%', left: 0, right: 0, marginBottom: 4, maxHeight: 'min(280px, 40vh)', overflow: 'auto', zIndex: 10, boxShadow: '0 8px 24px rgba(0,0,0,.28)' }}>
          <div className="sys-row" style={{ padding: '4px 10px' }}>
            {popup.kind === '@' ? t('composer.mentionHint') : t('composer.pathHint')}
          </div>
          {popup.items.map((it, i) => (
            <div
              key={it.label}
              id={`${popupId}-${i}`} role="option" aria-selected={i === sel}
              onMouseDown={event => event.preventDefault()}
              onClick={() => pick(it.label)}
              style={{
                padding: '5px 10px', cursor: 'pointer', display: 'flex', justifyContent: 'space-between',
                background: i === sel ? 'var(--bg-2)' : 'transparent',
                color: popup.kind === '@' ? 'var(--accent)' : 'var(--flag)',
              }}
            >
              <span className="mono" style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}>
                {popup.kind === '#' && <Icon name={it.label.endsWith('/') ? 'folder' : 'artifact'} size={11} />}
                {it.label}
              </span>
              <span className="dim3" style={{ fontSize: 11 }}>{it.hint}</span>
            </div>
          ))}
        </div>
      )}

        {/* ui-audit 票 10（P2-10）：单行 input → 自动增高 textarea（约 6 行上限内滚）。
            出处：Enter 行为变了——单行时代 Enter=发送是唯一语义；多行后
            Enter=发送、Shift+Enter=换行，用户习惯断层（粘 diff/多段指令进不来），
            placeholder 写明新键位。空输入 ↑ 回填发送历史（session 内存）。
            票 10：正文仍在 textarea（派活按 @ 分词不变）；.composer-mirror 把
            已收束的点名/路径画成原子块，退格逻辑在 onKeyDown。 */}
        <div className="composer-field">
          {/* 镜像在 textarea 之下：正文透明，光标画在上层才看得见。 */}
          <div ref={mirrorRef} className="composer-mirror" aria-hidden>
            <ComposerMirror text={text} atoms={atoms} />
          </div>
          <textarea
            id="composer-input"
            aria-autocomplete="list"
            aria-controls={popup?.items.length ? popupId : undefined}
            aria-activedescendant={popup?.items.length ? `${popupId}-${sel}` : undefined}
            ref={inputRef}
            rows={1}
            value={text}
            onChange={(e) => {
              onChange(e.target.value)
              fit(e.target)
            }}
            onScroll={(e) => {
              if (mirrorRef.current) mirrorRef.current.scrollTop = e.currentTarget.scrollTop
            }}
            onKeyDown={(e) => {
              if (popup && popup.items.length) {
                if (e.key === 'ArrowDown') { setSel((sel + 1) % popup.items.length); e.preventDefault(); return }
                if (e.key === 'ArrowUp') { setSel((sel - 1 + popup.items.length) % popup.items.length); e.preventDefault(); return }
                if (e.key === 'Enter' || e.key === 'Tab') { pick(popup.items[sel].label); e.preventDefault(); return }
                if (e.key === 'Escape') { setPopup(null); return }
              }
              // 票 10：打中原子块则一次删整块。修饰键留给系统逐词删除；
              // IME 组合中不拦截，否则中文输入的退格会吞掉已经点好的名字。
              if (
                (e.key === 'Backspace' || e.key === 'Delete') &&
                !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey &&
                !e.nativeEvent.isComposing
              ) {
                const ta = e.currentTarget
                const next = atomicDeletion(
                  text,
                  ta.selectionStart ?? 0,
                  ta.selectionEnd ?? 0,
                  e.key,
                  atoms,
                )
                if (next) {
                  e.preventDefault()
                  commitText(next.text, next.caret)
                  return
                }
              }
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                if (!mcpPending) void send()
                return
              }
              if (e.key === 'ArrowUp' && (!text || histIdx >= 0)) {
                e.preventDefault()
                const next = Math.min(histIdx + 1, HISTORY.length - 1)
                if (HISTORY[next] != null) { setText(HISTORY[next]); setHistIdx(next) }
                return
              }
              if (e.key === 'ArrowDown' && histIdx >= 0) {
                e.preventDefault()
                const next = histIdx - 1
                setHistIdx(next)
                setText(next >= 0 ? HISTORY[next] : '')
              }
            }}
            placeholder={mcpPending ? t('composer.initializing') : t('composer.placeholder')}
            disabled={mcpPending || submitting}
            // 票 03：粘贴图片（clipboardData.files）
            onPaste={(e) => {
              const files = Array.from(e.clipboardData?.files ?? [])
              if (files.length) {
                e.preventDefault() // 图不进文本流——走附件通道
                void addFiles(files)
              }
            }}
          />
        </div>
        <div className="composer-toolbar">
          <div className="composer-controls"><ProjectApprovalMode /><span className="composer-hint">{t('composer.multilineHint')}</span></div>
          <div className="composer-actions">
            <ContextMeter agentIds={team.filter((member) => atoms.some((atom) => atom.kind === 'mention'
              && (atom.value === member.role || atom.value === `${member.role}[${member.id}]`))).map((member) => member.id)} />
            <button className="btn primary composer-send" type="button"
              aria-label={submitting ? t('composer.sending') : t('composer.send')} aria-busy={submitting}
              title={`${t('composer.send')} · Enter`}
              disabled={mcpPending || submitting || (!text.trim() && attachments.length === 0 && elements.length === 0)} onClick={send}>
              <Icon name={submitting ? 'refresh' : 'arrow-up'} size={18} className={submitting ? 'composer-submit-spin' : undefined} />
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
