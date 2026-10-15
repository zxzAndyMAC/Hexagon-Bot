// 选区浮条的纯逻辑（ui-redesign 票 09，components D14 selbar；interaction §3「画布联动」）。
import { expect, it } from 'vitest'
import { linesOf, quoteSelection, selectionFill, selectionRef } from './canvasSelection'

const t = (key: string, o: Record<string, string>) => `${key}|${o.ref}${o.note ? `|${o.note}` : ''}`

it('引用头带行号区间，拿不到行号时只写路径', () => {
  expect(selectionRef({ path: 'src/a.ts', text: 'x', startLine: 3, endLine: 5 })).toBe('src/a.ts:3-5')
  expect(selectionRef({ path: 'src/a.ts', text: 'x', startLine: 3, endLine: 3 })).toBe('src/a.ts:3')
  expect(selectionRef({ path: 'README.md', text: 'x' })).toBe('README.md')
})

it('选区写成 Markdown 引用块，去掉首尾空行', () => {
  expect(quoteSelection('\nfoo\n\nbar\n')).toBe('> foo\n>\n> bar')
})

it('选区下标换成行号；整行选中带换行时不算下一行', () => {
  const text = 'a\nbb\nccc\n'
  expect(linesOf(text, 2, 4)).toEqual({ startLine: 2, endLine: 2 })
  expect(linesOf(text, 2, 5)).toEqual({ startLine: 2, endLine: 2 })
  expect(linesOf(text, 0, 7)).toEqual({ startLine: 1, endLine: 3 })
})

it('三个动作都只生成填进输入条的文字；描述为空的「描述修改」按「改写」处理', () => {
  const sel = { path: 'a.md', text: 'hello', startLine: 1, endLine: 1 }
  expect(selectionFill('rewrite', sel, t)).toBe('canvas.sel.rewriteFill|a.md:1\n> hello')
  expect(selectionFill('explain', sel, t)).toBe('canvas.sel.explainFill|a.md:1\n> hello')
  expect(selectionFill('describe', sel, t, ' 改成英文 ')).toBe('canvas.sel.describeFill|a.md:1|改成英文\n> hello')
  expect(selectionFill('describe', sel, t, '  ')).toBe('canvas.sel.rewriteFill|a.md:1\n> hello')
})
