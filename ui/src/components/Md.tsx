import { isValidElement, useEffect, useState, type ReactElement, type ReactNode } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { getHighlighter, useTheme } from '../highlight'

export function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const theme = useTheme()
  const [html, setHtml] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    getHighlighter()
      .then((hl) => {
        const l = lang && hl.getLoadedLanguages().includes(lang) ? lang : 'text'
        const out = hl.codeToHtml(code.replace(/\n$/, ''), {
          lang: l,
          theme: theme === 'dark' ? 'github-dark' : 'github-light',
        })
        if (live) setHtml(out)
      })
      .catch(() => {})
    return () => { live = false }
  }, [code, lang, theme])
  if (!html) return <pre className="mono shiki-fallback">{code}</pre>
  return <div className="shiki-wrap" dangerouslySetInnerHTML={{ __html: html }} />
}

const mdComponents = {
  // 围栏/缩进代码块一律走 CodeBlock；pre 层能看到 code 子的 language-* className
  pre: ({ children }: { children?: ReactNode }) => {
    const c = (Array.isArray(children) ? children[0] : children) as
      | ReactElement<{ className?: string; children?: ReactNode }>
      | undefined
    if (isValidElement(c)) {
      const m = /language-(\w+)/.exec(c.props.className ?? '')
      const raw = c.props.children
      const text = Array.isArray(raw) ? raw.join('') : String(raw ?? '')
      return <CodeBlock code={text.replace(/\n$/, '')} lang={m?.[1] ?? 'text'} />
    }
    return <pre>{children}</pre>
  },
  // 能走到这里的 code 只剩行内片段
  code: ({ children }: { children?: ReactNode }) => <code>{children}</code>,
}

/** GFM + 코드 하이라이트가 적용된 공용 마크다운 렌더러. */
export function Md({ children }: { children: string }) {
  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>
      {children}
    </ReactMarkdown>
  )
}
