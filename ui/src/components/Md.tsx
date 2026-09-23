import { isValidElement, memo, useEffect, useRef, useState, type ReactElement, type ReactNode } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { useTranslation } from 'react-i18next'
import { getHighlighter, useTheme } from '../highlight'
import { errText } from '../api'
import { useUiStore } from '../store'
import { Icon } from './Icon'

// beautiful-ui 票 08：块级代码头部条（语言标签 + 复制钮）——
// 只包 CodeBlock，行内 code 不出头；高亮体仍是 shiki，未就绪时 fallback
// 也带头（复制不依赖高亮）。
// 时间线是虚拟列表（Virtuoso）——行滚出视口即卸载，滚回重挂。
// 高亮结果模块级缓存：重挂载直接从 cache 起 html，不再闪「未高亮 pre →
// 异步染色」两帧（owner 反馈：内容多时快速滚动闪烁抖动）。
const highlightCache = new Map<string, string>()
const HIGHLIGHT_CACHE_CAP = 400

export function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const theme = useTheme()
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const cacheKey = `${theme}|${lang ?? ''}|${code}`
  const [html, setHtml] = useState<string | null>(() => highlightCache.get(cacheKey) ?? null)
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout>>(null)
  useEffect(() => {
    if (highlightCache.has(cacheKey)) return
    let live = true
    getHighlighter()
      .then((hl) => {
        const l = lang && hl.getLoadedLanguages().includes(lang) ? lang : 'text'
        const out = hl.codeToHtml(code.replace(/\n$/, ''), {
          lang: l,
          theme: theme === 'dark' ? 'github-dark' : 'github-light',
        })
        // LRU 近似：写满先清一半（Map 保插入序，重插即续龄）
        if (highlightCache.size >= HIGHLIGHT_CACHE_CAP) {
          for (const k of [...highlightCache.keys()].slice(0, HIGHLIGHT_CACHE_CAP / 2)) highlightCache.delete(k)
        }
        highlightCache.set(cacheKey, out)
        if (live) setHtml(out)
      })
      .catch(() => {})
    return () => { live = false }
  }, [cacheKey, code, lang, theme])
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current) }, [])
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code.replace(/\n$/, ''))
      setCopied(true)
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(() => setCopied(false), 1500)
    } catch (e) {
      pushToast(errText(e), 'err')
    }
  }
  return (
    <div className="code-block">
      <div className="code-head">
        <span className="code-lang mono">{lang ?? 'text'}</span>
        <button className="icon-btn code-copy" onClick={copy}>
          <Icon name={copied ? 'check' : 'copy'} size={11} />
          {copied ? t('md.copied') : t('md.copy')}
        </button>
      </div>
      {!html
        ? <pre className="mono shiki-fallback">{code}</pre>
        : <div className="shiki-wrap" dangerouslySetInnerHTML={{ __html: html }} />}
    </div>
  )
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
// memo：时间线 atBottom 翻转/flash 等父级重渲时跳过 remark 重解析
//（重挂载仍要解析——组件实例不跨卸载存活，配合 Virtuoso overscan 摊掉）。
export const Md = memo(function Md({ children }: { children: string }) {
  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>
      {children}
    </ReactMarkdown>
  )
})
