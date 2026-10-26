// 时间线消息（C1 Agent / C2 负责人）的动作（components A16b 第一批「消息」，owner-followups-1015 #08）。
// 右键菜单、命令面板、keymap 三个入口共用这里：复制 · 引用到输入条 · 在节点轨中定位。
// 作用范围：窗口选区落在该消息内 → 只作用于选区；否则作用于整条（整条复制的是消息原文 Markdown，选区复制的是看到的字）。
// 引用复用画布选区的 quoteSelection + sendSelectionToComposer（只填不发）。
import { createContext } from 'react'
import i18n from './i18n'
import { quoteSelection, sendSelectionToComposer } from './canvasSelection'
import { useUiStore } from './store'
import { toastError } from './toast'

export type MessageAction = 'copy' | 'quote' | 'locate'

/** 节点轨上有刻度的事件 id（Timeline 按当前刻度提供，消息菜单据此决定「定位」可不可用）。 */
export const RailEvents = createContext<ReadonlySet<number>>(new Set())

/** 消息宿主（MessageContextMenu 的触发器）接收动作的 DOM 事件：命令面板与键位只知道焦点在哪条消息上，
 * 消息原文在宿主组件里，所以由宿主执行（detail = 动作）。 */
export const MESSAGE_ACTION_EVENT = 'hexagon:message-action'

/** 节点 → 所在消息宿主（`[data-msg-event]`）。 */
export function messageHostOf(node: Node | null | undefined): HTMLElement | null {
  const el = node instanceof Element ? node : node?.parentElement
  return el?.closest<HTMLElement>('[data-msg-event]') ?? null
}

/** 键位 / 命令面板的目标消息：焦点所在的消息，否则选区所在的消息。 */
export function messageTarget(focused: Element | null = document.activeElement): HTMLElement | null {
  return messageHostOf(focused) ?? messageHostOf(window.getSelection()?.anchorNode)
}

/** 该消息在节点轨上有刻度（宿主写 data-msg-on-rail）。 */
export const messageOnRail = (host: HTMLElement) => host.dataset.msgOnRail != null

/** 选区两端都在 host 内且非空 → 选中的文字；否则 null（作用于整条）。 */
export function selectionWithin(host: Element, sel: Selection | null = window.getSelection()): string | null {
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) return null
  if (!sel.anchorNode || !sel.focusNode || !host.contains(sel.anchorNode) || !host.contains(sel.focusNode)) return null
  const text = sel.toString()
  return text.trim() ? text : null
}

export function dispatchMessageAction(host: HTMLElement, action: MessageAction) {
  host.dispatchEvent(new CustomEvent<MessageAction>(MESSAGE_ACTION_EVENT, { detail: action }))
}

/** 复制；toast 写明范围（整条 / 选区），失败走 toastError（A16b 错误态：菜单已关）。 */
export async function copyMessageText(text: string, scoped: boolean) {
  try {
    await navigator.clipboard.writeText(text)
    useUiStore.getState().pushToast(i18n.t(scoped ? 'ctxMessage.selectionCopied' : 'ctxMessage.messageCopied'), 'ok')
  } catch (e) {
    toastError(e)
  }
}

/** 引用到输入条：Markdown 引用块 + 空一行——不空行的话负责人接着写的字会被当成引用块的延续行（CommonMark 惰性续行）。 */
export function quoteMessage(text: string) {
  sendSelectionToComposer(`${quoteSelection(text)}\n\n`)
}

/** 节点轨上这条消息的刻度（单道 data-tick-event，合并刻度 data-tick-events 列出全部）。 */
function railTick(eventId: number): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-node-rail] [data-tick-event="${eventId}"], [data-node-rail] [data-tick-events~="${eventId}"]`)
}

/** 在节点轨中定位：聚焦该刻度（出预览卡，窄窗收起时键盘聚焦也会显示轨道）。下一帧做，理由同 mentionMember。 */
export function locateInRail(eventId: number) {
  requestAnimationFrame(() => railTick(eventId)?.focus())
}
