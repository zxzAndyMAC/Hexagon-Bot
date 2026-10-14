// 选区来源（components D14 selbar）：DOM 选区（Markdown 预览、产物代码视图）与 textarea 选区。
// Monaco 的选区由 monacoFile.ts 的 onSelection 回调给出，同样折成 SelectionAnchor。
import { useEffect, type RefObject } from 'react'
import { linesOf, type CanvasSelection } from '../../canvasSelection'

export type SelectionAnchor = { sel: CanvasSelection; x: number; y: number; bottom: number }

/**
 * DOM 选区（Markdown 预览、产物代码视图）：松开鼠标 / 键盘选择后读 window.getSelection()，
 * 只认落在 root 内的选区。代码视图行带 data-line，能给出行号区间。
 */
export function useDomSelection(root: RefObject<HTMLElement | null>, path: string, onPick: (a: SelectionAnchor | null) => void) {
  useEffect(() => {
    const el = root.current
    if (!el) return
    const read = () => {
      const s = window.getSelection()
      if (!s || s.isCollapsed || s.rangeCount === 0) { onPick(null); return }
      const range = s.getRangeAt(0)
      if (!el.contains(range.commonAncestorContainer)) return
      const text = s.toString()
      if (!text.trim()) { onPick(null); return }
      const line = (n: Node | null) => {
        const host = n instanceof Element ? n : n?.parentElement
        const v = host?.closest('[data-line]')?.getAttribute('data-line')
        return v ? Number(v) : undefined
      }
      const a = line(range.startContainer)
      const b = line(range.endContainer)
      const r = range.getBoundingClientRect()
      const box = el.getBoundingClientRect()
      onPick({
        sel: { path, text, startLine: a, endLine: b ?? a },
        x: r.left - box.left, y: r.top - box.top, bottom: r.bottom - box.top,
      })
    }
    const onKey = (e: KeyboardEvent) => { if (e.shiftKey) read() }
    el.addEventListener('mouseup', read)
    el.addEventListener('keyup', onKey)
    return () => { el.removeEventListener('mouseup', read); el.removeEventListener('keyup', onKey) }
  }, [root, path, onPick])
}

/** textarea 选区（测试环境与无 Monaco 时的编辑器）：selectionStart/End 换行号，浮条放在指针处。 */
export function textareaSelection(ta: HTMLTextAreaElement, path: string, root: HTMLElement, pointer?: { x: number; y: number }): SelectionAnchor | null {
  const { selectionStart: s, selectionEnd: e, value } = ta
  if (s === e || !value.slice(s, e).trim()) return null
  const box = root.getBoundingClientRect()
  const at = pointer ?? { x: box.left + 24, y: box.top + 24 }
  return { sel: { path, text: value.slice(s, e), ...linesOf(value, s, e) }, x: at.x - box.left, y: at.y - box.top, bottom: at.y - box.top }
}

