// 画布代码视图的语法着色（components D15；foundations §1.5「代码语法色」`--tk-k/s/f/c`）。
// 用 Shiki 的 css-variables 主题拿 token 类别，再映射到规范的四个语法色——不用 github 主题的内联色，
// 那套颜色不在 token 表里，深浅切换也不跟主题走。首帧未染色时调用方显示等宽纯文本（D15 加载行）。
// 结果按 lang|text 缓存（LRU 近似，同 Md CodeBlock 的高亮缓存口径）。
import { useEffect, useState } from 'react'
import { createCssVariablesTheme } from 'shiki'
import { getHighlighter } from './highlight'

export type CodeToken = { content: string; color?: string }

const THEME = 'hexagon-tokens'
const PREFIX = '--hx-code-'
const theme = createCssVariablesTheme({ name: THEME, variablePrefix: PREFIX })

/** Shiki css-variables 类别 → 规范语法色；其余类别（变量、标点、常量）随正文 --ink。 */
// 键名拼出来写：Shiki 主题回传的是 `var(<前缀>token-keyword)` 这样的字面串，这里不是 token 引用（check-tokens 只认 index.css 定义）。
const CAT = (name: string) => `var(${PREFIX}${name})`
const COLOR: Record<string, string> = {
  [CAT('token-keyword')]: 'var(--tk-k)',
  [CAT('token-string')]: 'var(--tk-s)',
  [CAT('token-string-expression')]: 'var(--tk-s)',
  [CAT('token-function')]: 'var(--tk-f)',
  [CAT('token-comment')]: 'var(--tk-c)',
}

const CACHE_CAP = 64
const cache = new Map<string, CodeToken[][]>()
let themeLoaded: Promise<void> | null = null

export async function tokenize(text: string, lang: string): Promise<CodeToken[][]> {
  const key = `${lang}|${text}`
  const hit = cache.get(key)
  if (hit) return hit
  const hl = await getHighlighter()
  themeLoaded ??= hl.loadTheme(theme)
  await themeLoaded
  const l = hl.getLoadedLanguages().includes(lang) ? lang : 'text'
  const lines = hl.codeToTokensBase(text, { lang: l as never, theme: THEME as never })
    .map((line) => line.map((tk) => ({ content: tk.content, color: tk.color ? COLOR[tk.color] : undefined })))
  if (cache.size >= CACHE_CAP) for (const k of [...cache.keys()].slice(0, CACHE_CAP / 2)) cache.delete(k)
  cache.set(key, lines)
  return lines
}

/** 着色结果；未就绪或失败时为 null（调用方显示纯文本）。 */
export function useCodeTokens(text: string | null, lang: string): CodeToken[][] | null {
  const key = text == null ? null : `${lang}|${text}`
  const [state, setState] = useState<{ key: string | null; lines: CodeToken[][] | null }>(() => ({
    key, lines: key ? cache.get(key) ?? null : null,
  }))
  useEffect(() => {
    if (text == null || key == null) return
    let live = true
    tokenize(text, lang).then((lines) => { if (live) setState({ key, lines }) }).catch(() => {})
    return () => { live = false }
  }, [key, text, lang])
  if (key == null) return null
  return state.key === key ? state.lines : cache.get(key) ?? null
}
