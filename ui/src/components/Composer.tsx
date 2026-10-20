import { ComputerTaskReadiness } from './ComputerTaskReadiness'
import { ElementDraft } from './ElementReferences'
import type { ElementRef } from '../gen/ElementRef'
import { ProjectApprovalMode } from './ProjectApprovalMode'
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react'
import { ComposerSuggestions } from './ComposerSuggestions'
import { popupVisible, type SuggestPopup } from '../composerSuggest'
import { IconButton } from '@/components/ui/icon-button'
import { TooltipProvider } from '@/components/ui/tooltip'
import { Spinner } from '@/components/ui/spinner'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { bindingFor, formatBinding } from '../keymap'
import './composer.css'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import { api, errText } from '../api'
import { Icon } from './Icon'
import { ContextMeter } from './ContextMeter'
import { COMPOSER_FILL_EVENT, COMPOSER_MENTION_EVENT, mergeFill, useFreshStartShown } from '../composerFill'
import { visiblePending } from '../decisions'
import type { AttachRef } from '../gen/AttachRef'
import {
  atomicDeletion,
  findAtoms,
  insertPathToken,
  mentionName as mentionNameOf,
  mentionTokenText,
  insertToken,
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
  const freshShown = useFreshStartShown()
  // 票 16a（decisions#dock）：坞展开且有卡时，占位改为「先处理上面的待决，或继续给团队发消息」——
  // 输入条照常可用（团队运行不阻止负责人发言），只提示先看坞；收起成胶囊后回到常规占位（decisions#collapsed）。
  const dockShown = useUiStore((s) => s.pendingDialogOpen
    && (s.reviewRows.length > 0 || visiblePending(s.pending, s.hiddenPending).length > 0))
  const team = useUiStore((s) => s.team)
  const invalidate = useUiStore((s) => s.invalidate)
  const pushToast = useUiStore((s) => s.pushToast)
  const mcpPending = useUiStore((s) => s.mcpPending)
  const [submitting, setSubmitting] = useState(false)
  const submittingRef = useRef(false)
  const [text, setText] = useState('')
  const [popup, setPopup] = useState<SuggestPopup | null>(null)
  const artifacts = useUiStore((s) => s.artifacts)
  const fileInput = useRef<HTMLInputElement>(null)
  const pathQuery = useRef('')
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
  const mentionName = (m: typeof team[number]) => mentionNameOf(m, team)
  const roster = useMemo(() => new Set(team.flatMap((m) => [m.role, `${m.role}[${m.id}]`])), [team])
  const atoms = useMemo(() => findAtoms(text, roster), [text, roster])

  const fetchPaths = (q: string) => {
    if (pathTimer.current) clearTimeout(pathTimer.current)
    pathQuery.current = q
    const seq = ++pathSeq.current
    pathTimer.current = setTimeout(() => {
      api.repoPaths(q)
        .then((ps) => {
          if (pathSeq.current !== seq) return // 晚到的旧查询不盖新结果
          // 产物路径在右侧标「产物 vN」（composer#path）；其余文件不写类型
          const versions = new Map(artifacts.map((a) => [a.path, a.version]))
          setPopup({
            kind: '#',
            items: ps.map((p) => ({
              kind: '#' as const, label: `#${p}`, path: p, dir: p.endsWith('/'), artifact: versions.get(p) ?? null,
            })),
          })
          setSel(0)
        })
        // A16 错误态：菜单内单行红字 + 「重试」（旧行为是静默关掉弹层）
        .catch(() => { if (pathSeq.current === seq) setPopup({ kind: '#', items: [], error: true }) })
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
          .map((m) => {
            const name = mentionName(m)
            return {
              kind: '@' as const, label: mentionTokenText(name), agentId: m.id, name: m.role,
              inst: name === m.role ? null : m.id, sleeping: m.status === 'sleeping', duty: m.duty,
            }
          }),
      })
      setSel(0)
    } else if (last.startsWith('#')) {
      // components A16 加载态：首屏出骨架；防抖期间保持旧结果，不闪成空壳
      setPopup((prev) => ({ kind: '#', items: prev?.kind === '#' ? prev.items : [], loading: true }))
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
      // components D1 加载态：发送中输入保持可编辑。发送期间新打的字留下（去掉已发出的那段前缀），
      // 草稿没动过就清空——旧版发送中锁死输入框，负责人想接着写下一句只能干等。
      const rest = (cur: string) => (cur.startsWith(body) ? cur.slice(body.length).replace(/^\s+/, '') : cur)
      setText(rest)
      if (inputRef.current) { inputRef.current.style.height = 'auto'; fit(inputRef.current) }
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

  // ui-redesign 票 05：归来摘要建议 / 首屏建议卡只填入草稿、聚焦，不发送。
  useEffect(() => {
    const onFill = (e: Event) => {
      const fill = (e as CustomEvent<unknown>).detail
      if (typeof fill !== 'string' || !fill) return
      setText((draft) => {
        const next = mergeFill(draft, fill)
        pendingCaret.current = next.length
        return next
      })
      setHistIdx(-1)
      setPopup(null)
      inputRef.current?.focus()
    }
    window.addEventListener(COMPOSER_FILL_EVENT, onFill)
    return () => window.removeEventListener(COMPOSER_FILL_EVENT, onFill)
  }, [])

  // owner-followups-1015 #08：成员行右键「@ 点名」——未聚焦插到文末，聚焦插在光标处（同拖入路径块的口径）。
  useEffect(() => {
    const onMention = (e: Event) => {
      const token = (e as CustomEvent<unknown>).detail
      if (typeof token !== 'string' || !token) return
      const ta = inputRef.current
      const caret = ta && document.activeElement === ta ? ta.selectionStart : null
      setText((draft) => {
        const next = insertToken(draft, caret ?? draft.length, token)
        pendingCaret.current = next.caret
        return next.text
      })
      setHistIdx(-1)
      setPopup(null)
    }
    window.addEventListener(COMPOSER_MENTION_EVENT, onMention)
    return () => window.removeEventListener(COMPOSER_MENTION_EVENT, onMention)
  }, [])

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

  const mentionIds = team.filter((member) => atoms.some((atom) => atom.kind === 'mention'
    && (atom.value === member.role || atom.value === `${member.role}[${member.id}]`))).map((member) => member.id)
  const empty = !text.trim() && attachments.length === 0 && elements.length === 0
  const attachBinding = formatBinding(bindingFor('composerAttach'))

  // ui-redesign 票 07（components D1，composer#mention/path/attach/mode）：
  // composer-wrap → 预检卡（D19，输入条正上方）→ 工具行 → 输入条（引用区 D3 → 输入 → 底栏）→ 引用说明。
  // 列宽读 --col-w（画布并排 560 / 否则 720，App 的对话主区挂），与时间线同列。
  return (
    <TooltipProvider>
    <div
      data-testid="composer"
      className="composer-wrap"
      data-dragging={dragging || undefined}
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
      <div className="composer-above">
        <ComputerTaskReadiness mentions={atoms.filter(a => a.kind === 'mention').map(a => a.value)} onSend={() => void send()} />
      </div>
      <ElementDraft references={elements} onChange={setElements} submitting={submitting}>{({ tools, list, preview, status }) => <>
      <div className="composer-toolrow">{tools}{status}</div>
      {/* Owner issue16: suggestions anchor to the input box, not above the reference shelf. */}
      <div className="composer composer-box">
        {popupVisible(popup) && <ComposerSuggestions ref={suggestions} id={popupId} popup={popup} sel={sel}
          onPick={pick} onHover={setSel} onRetry={() => { setPopup({ kind: '#', items: [], loading: true }); fetchPaths(pathQuery.current) }} />}
        {/* 票 03 + 票 10：按图显示。关闭钮在图上，只去掉这一张；点图放大（A17 Dialog）。 */}
        {(attachments.length > 0 || list) && (
          <div className="composer-refs">
            {attachments.map((a) => (
              <div key={a.path} className="attach-chip">
                <button type="button" className="attach-thumb" aria-label={t('composer.imageZoom', { name: a.name })} onClick={() => setZoom(a)}>
                  <img src={a.preview} alt={a.name} />
                </button>
                <button type="button" className="attach-remove" aria-label={t('composer.attachRemove')} onClick={() => void removeAttachment(a)}>
                  <Icon name="close" size={12} />
                </button>
              </div>
            ))}
            {list}
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
              aria-controls={popupVisible(popup) ? popupId : undefined}
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
                // 加载 / 错误中的弹层也吃 Esc（关掉）；↑↓↵ 只在有结果时接管
                if (popupVisible(popup) && !popup.items.length && e.key === 'Escape') { setPopup(null); return }
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
              placeholder={mcpPending ? t('composer.initializing') : t(dockShown ? 'composer.placeholderPending' : freshShown ? 'composer.placeholderFresh' : 'composer.placeholder')}
              disabled={mcpPending}
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
        <div className="composer-bar">
          <IconButton label={t('composer.attach')} action={attachBinding ? 'composerAttach' : undefined}
            disabled={submitting || mcpPending || attachments.length >= ATTACH_MAX}
            disabledReason={attachments.length >= ATTACH_MAX ? t('composer.attachTooMany', { max: ATTACH_MAX }) : undefined}
            onClick={() => fileInput.current?.click()}><Icon name="plus" size={16} /></IconButton>
          <ProjectApprovalMode />
          <span className="sp" />
          <ContextMeter agentIds={mentionIds} />
          <button className="composer-send" type="button"
            aria-label={submitting ? t('composer.sending') : t('composer.send')} aria-busy={submitting}
            title={`${t('composer.send')} · ↵`}
            disabled={mcpPending || submitting || empty} onClick={send}>
            {submitting ? <Spinner /> : <Icon name="arrow-up" size={16} />}
          </button>
        </div>
      </div>
      {elements.length > 0 && <div className="composer-hint-foot">{t('composer.refHint')}</div>}
      {preview}
      </>}</ElementDraft>
      <input ref={fileInput} type="file" hidden multiple accept={Object.keys(IMG_EXT).join(',')}
        onChange={(e) => { const files = Array.from(e.target.files ?? []); e.target.value = ''; void addFiles(files) }} />
      {/* components A17 图片放大（Dialog，不触发裁决快捷键）；关掉回到输入框 */}
      <Dialog open={!!zoom} onOpenChange={(open) => { if (!open) setZoom(null) }}>
        {zoom && <DialogContent className="attach-zoom w-auto max-w-[90vw] p-3" aria-describedby={undefined}
          onCloseAutoFocus={(e) => { e.preventDefault(); inputRef.current?.focus() }}>
          <DialogTitle className="sr-only">{t('composer.imageZoom', { name: zoom.name })}</DialogTitle>
          <img src={zoom.preview} alt={zoom.name} className="block max-h-[80vh] max-w-[86vw] rounded-[var(--r-card)]" />
          <IconButton ref={zoomCloseRef} className="attach-zoom-close absolute top-2 right-2 bg-surface" label={t('composer.previewClose')}
            onClick={() => setZoom(null)}><Icon name="close" size={14} /></IconButton>
        </DialogContent>}
      </Dialog>
    </div>
    </TooltipProvider>
  )
}
