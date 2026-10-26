// 消息右键菜单（components A16b 第一批「消息 C1 / C2」，owner-followups-1015 #08）：复制 · 引用到输入条 · 在节点轨中定位。
// - 范围：右键时窗口选区落在该消息内 → 复制 / 引用只作用于选区，否则作用于整条（原文 Markdown）。
// - 「定位」只对节点轨上有刻度的消息显示（负责人发言是 m 刻度；Agent 消息不上轨）。票 08 原为常驻、禁用写「不在节点轨上」，
//   2026-10-20 负责人裁决（票 29）：没有刻度就不显示——同 A16b「该对象种类根本没有这个动作 → 不显示」。
// - 文案跟范围走（票 29）：右键时有选区 → 「复制所选」「引用所选」；无选区 → 「复制」「引用到输入条」。
// - 禁区：消息菜单里没有任何裁决项；待决卡 / 裁决按钮不挂右键菜单（A16b「禁区」，误点代价是一次未审副作用）。
// - 文件目标（#32，2026-10-21 负责人裁决「凡是显示有盘上路径的文件，右键都给同一份文件菜单」）：右键落在 # 路径块
//   （`data-atom-path`）或附件 chip（`data-file-path`）上 → 这一次菜单换成共用文件菜单（FileMenu），不出消息项；
//   被否决：在路径块里再套一层 ContextMenu——contextmenu 冒泡会把外层消息菜单一起打开，两层菜单叠在指针处。
// - 命令面板与键位经 MESSAGE_ACTION_EVENT 让宿主执行（原文只在这里），同一套动作。
import { useContext, useEffect, useRef, useState, type HTMLAttributes, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { isMac } from '../keymap'
import {
  copyMessageText, locateInRail, MESSAGE_ACTION_EVENT, quoteMessage, RailEvents, selectionWithin, type MessageAction,
} from '../messageActions'
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/context-menu'
import { useUiStore } from '../store'
import { FileMenuItems, useProjectFileTarget } from './FileMenu'

/** 右键 mousedown 记下的选区多久内算数（之后到的 contextmenu 用它；键盘打开的没有 mousedown，现读选区）。 */
const SNAPSHOT_MS = 1000

export function MessageContextMenu({ eventId, body, children, ...rest }: {
  eventId: number
  /** 整条作用时的原文。 */
  body: string
  children: ReactNode
} & HTMLAttributes<HTMLDivElement>) {
  const { t } = useTranslation()
  const onRail = useContext(RailEvents).has(eventId)
  const hostRef = useRef<HTMLDivElement>(null)
  const snapshot = useRef<{ text: string | null; at: number } | null>(null)
  const [scoped, setScoped] = useState<string | null>(null)
  // 右键落点上的文件（# 路径块 / 附件 chip）；null = 消息菜单。
  const [fileHit, setFileHit] = useState<{ path: string; atom: boolean } | null>(null)
  // 引用 / 定位把焦点送去输入条 / 刻度：菜单关闭时不还焦点给消息，否则盖掉动作的聚焦。
  const skipRestore = useRef(false)

  const run = (action: MessageAction, selected: string | null) => {
    if (action === 'copy') void copyMessageText(selected ?? body, selected != null)
    else if (action === 'quote') quoteMessage(selected ?? body)
    else if (onRail) locateInRail(eventId)
  }
  // 宿主上的 DOM 监听只挂一次，经 ref 读最新的原文 / 上轨状态。
  const runRef = useRef(run)
  useEffect(() => { runRef.current = run })

  // 命令面板 / 键位入口：选区在执行时现读。
  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    const onAction = (e: Event) => {
      const action = (e as CustomEvent<unknown>).detail
      if (action === 'copy' || action === 'quote' || action === 'locate') runRef.current(action, selectionWithin(host))
    }
    host.addEventListener(MESSAGE_ACTION_EVENT, onAction)
    return () => host.removeEventListener(MESSAGE_ACTION_EVENT, onAction)
  }, [])

  const pick = (action: MessageAction, movesFocus: boolean) => () => {
    skipRestore.current = movesFocus
    run(action, scoped)
  }

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div ref={hostRef} data-msg-event={eventId} data-msg-on-rail={onRail ? '' : undefined} {...rest}
          onMouseDownCapture={(e) => {
            if (e.button !== 2 && !(isMac && e.ctrlKey)) return
            // WKWebView 右键按下会先把光标下的词选中，等 contextmenu 到时选区已变成那个词——
            // 每次右键都会只作用于一个词。按下前记下原选区，并拦掉按下的默认动作（右键不改选区）。
            snapshot.current = { text: selectionWithin(e.currentTarget), at: Date.now() }
            e.preventDefault()
          }}
          onContextMenu={(e) => {
            // 键盘打开（Shift+F10）时 contextmenu 派发在宿主上，落点取焦点所在（焦点在路径块按钮上 = 那个文件）
            setFileHit(fileAt(e.target === e.currentTarget ? document.activeElement : e.target))
            const snap = snapshot.current
            snapshot.current = null
            setScoped(snap && Date.now() - snap.at < SNAPSHOT_MS ? snap.text : selectionWithin(e.currentTarget))
          }}>
          {children}
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent data-msg-menu={eventId}
        onCloseAutoFocus={(e) => { if (skipRestore.current) { skipRestore.current = false; e.preventDefault() } }}>
        {fileHit ? <MessageFileItems hit={fileHit} /> : <>
        <ContextMenuItem data-ctx-item="copy" action="copyMessage" onSelect={pick('copy', false)}>
          {t(scoped != null ? 'ctxMessage.copySelection' : 'ctxMessage.copy')}
        </ContextMenuItem>
        <ContextMenuItem data-ctx-item="quote" action="quoteMessage" onSelect={pick('quote', true)}>
          {t(scoped != null ? 'ctxMessage.quoteSelection' : 'ctxMessage.quote')}
        </ContextMenuItem>
        {onRail && (
          <ContextMenuItem data-ctx-item="locate" action="locateInRail" onSelect={pick('locate', true)}>
            {t('ctxMessage.locate')}
          </ContextMenuItem>
        )}
        </>}
      </ContextMenuContent>
    </ContextMenu>
  )
}

/** 右键落点 → 文件目标：# 路径块（`data-atom-path`，可「打开」）或附件 chip（`data-file-path`，本表面无打开动作）。 */
function fileAt(target: EventTarget | null): { path: string; atom: boolean } | null {
  const el = target instanceof Element ? target : null
  const atom = el?.closest<HTMLElement>('[data-atom-path]')?.dataset.atomPath
  if (atom) return { path: atom, atom: true }
  const file = el?.closest<HTMLElement>('[data-file-path]')?.dataset.filePath
  return file ? { path: file, atom: false } : null
}

function MessageFileItems({ hit }: { hit: { path: string; atom: boolean } }) {
  const dir = hit.path.endsWith('/')
  const path = dir ? hit.path.slice(0, -1) : hit.path
  // 「打开」同点击路径块（Timeline OwnerMessage.onAtom）：文件开文件页签；目录没有「打开」（同文件树行）
  const onOpen = hit.atom && !dir
    ? () => useUiStore.getState().openTab({ id: `file:${path}`, kind: 'file', title: path.split('/').pop() || path, path })
    : undefined
  const target = useProjectFileTarget(path, { onOpen, kind: dir ? 'dir' : 'file' })
  return <FileMenuItems {...target} />
}
