// 画布选区浮条（components D14 `selbar`，canvas#file；interaction §3「画布联动」、§6）的纯逻辑。
// 「解释 / 改写 / 描述修改」三个动作都只把选区作为引用填进输入条（fillComposer），由负责人决定发给谁——
// 画布里没有任何直接发送的路径（spec 用户故事 54）。被否决：「改写」直接派给作者 Agent——
// 绕过输入条就绕过了点名与批准模式，负责人看不到发出去的是什么。

export type CanvasSelection = {
  /** 仓库相对路径（文件）或产物路径。 */
  path: string
  text: string
  /** 1 起的行号区间；拿不到（Markdown 预览）时省略。 */
  startLine?: number
  endLine?: number
}

export type SelectionAction = 'explain' | 'rewrite' | 'describe'

/** 引用头：`path:3` / `path:3-5` / `path`。 */
export function selectionRef(sel: CanvasSelection): string {
  if (sel.startLine == null) return sel.path
  const end = sel.endLine ?? sel.startLine
  return end > sel.startLine ? `${sel.path}:${sel.startLine}-${end}` : `${sel.path}:${sel.startLine}`
}

/** 选区按 Markdown 引用块写出（每行前缀 `> `），去掉首尾空行。 */
export function quoteSelection(text: string): string {
  return text.replace(/^\s*\n|\n\s*$/g, '').split('\n').map((l) => (l ? `> ${l}` : '>')).join('\n')
}

/** 给定文本与选区下标，算出 1 起的行号区间（textarea / Monaco 外的通用口径）。 */
export function linesOf(text: string, start: number, end: number): { startLine: number; endLine: number } {
  const lineAt = (i: number) => text.slice(0, Math.max(0, i)).split('\n').length
  // 选区止于行首（整行选中带换行）时不把下一行算进去
  const last = end > start && text[end - 1] === '\n' ? end - 1 : end
  return { startLine: lineAt(start), endLine: lineAt(last) }
}

/**
 * 填进输入条的文字。动作前缀走 i18n（t 由调用方传入），引用块原样保留——项目内容不翻译（AGENTS.md）。
 * 「描述修改」没写描述时等同「改写」。
 */
export function selectionFill(
  action: SelectionAction,
  sel: CanvasSelection,
  t: (key: string, opts: Record<string, string>) => string,
  note = '',
): string {
  const ref = selectionRef(sel)
  const quote = quoteSelection(sel.text)
  const desc = note.trim()
  if (action === 'describe' && desc) return `${t('canvas.sel.describeFill', { ref, note: desc })}\n${quote}`
  const key = action === 'explain' ? 'canvas.sel.explainFill' : 'canvas.sel.rewriteFill'
  return `${t(key, { ref })}\n${quote}`
}
