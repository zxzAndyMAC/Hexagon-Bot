// 代码视图 CodeView / 统一 diff（components D15，canvas#artifact、workbench#resize）。
// 12/1.75 等宽，padding 10 0；行号列 44 宽右对齐 ink-3、右距 14、不可选；新增行 --green-tint 底、行号 green；
// 删除行 --red-tint 底、行号 red；语法色 foundations §1.5（codeTokens.ts）。行悬停 --hover（补设计）。
// 容器可聚焦以键盘滚动（焦点环）。旧 DiffView（红绿 +/- 行）仍给执行卡与待决卡用，不在本票范围。
// 增删不只靠颜色（accessibility）：行内留一个读屏可读的 +/− 记号。
import { useMemo, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { numberDiff, type DiffOp } from '../../diff'
import { useCodeTokens, type CodeToken } from '../../codeTokens'
import { cn } from '@/lib/utils'

const LINE = 'flex min-w-max pr-3.5 live:hover:bg-hover'
const NUM = 'inline-block w-11 shrink-0 pr-3.5 text-right text-ink-3 select-none'

function Tokens({ tokens, text }: { tokens: CodeToken[] | undefined; text: string }) {
  if (!tokens) return <>{text}</>
  return <>{tokens.map((tk, i) => <span key={i} style={tk.color ? { color: tk.color } : undefined}>{tk.content}</span>)}</>
}

function Frame({ children, label }: { children: ReactNode; label: string }) {
  return (
    // 容器可聚焦以键盘滚动（D15 聚焦行）
    // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex
    <div tabIndex={0} role="region" aria-label={label} data-code-view
      className="hx-focus min-h-0 flex-1 overflow-auto py-2.5 font-mono text-[12px] leading-[1.75] text-ink">
      {children}
    </div>
  )
}

/** 只读代码（带行号与语法色）。 */
export function CodeView({ text, lang, label }: { text: string; lang: string; label?: string }) {
  const { t } = useTranslation()
  const tokens = useCodeTokens(text, lang)
  const lines = useMemo(() => text.replace(/\n$/, '').split('\n'), [text])
  return (
    <Frame label={label ?? t('canvas.code')}>
      {lines.map((l, i) => (
        <div key={i} className={LINE} data-line={i + 1}>
          <i className={cn(NUM, 'not-italic')}>{i + 1}</i>
          <span className="whitespace-pre"><Tokens tokens={tokens?.[i]} text={l} /></span>
        </div>
      ))}
    </Frame>
  )
}

/** 统一 diff：旧 / 新两侧分别着色，删除行取旧文件 token、其余取新文件 token。 */
export function DiffCode({ ops, before, after, lang, label }: {
  ops: DiffOp[]
  before: string
  after: string
  lang: string
  label?: string
}) {
  const { t } = useTranslation()
  const rows = useMemo(() => numberDiff(ops), [ops])
  const a = useCodeTokens(before, lang)
  const b = useCodeTokens(after, lang)
  return (
    <Frame label={label ?? t('canvas.diff')}>
      {rows.map((r, i) => (
        <div key={i} data-diff={r.type}
          className={cn(LINE, r.type === 'ins' && 'bg-green-tint', r.type === 'del' && 'bg-red-tint')}>
          <i className={cn(NUM, 'not-italic', r.type === 'ins' && 'text-green-ink', r.type === 'del' && 'text-red-ink')}>{r.line}</i>
          {r.type !== 'eq' && <span className="sr-only">{r.type === 'ins' ? '+' : '-'}</span>}
          <span className="whitespace-pre">
            <Tokens tokens={r.type === 'del' ? a?.[r.a!] : b?.[r.b!]} text={r.text} />
          </span>
        </div>
      ))}
    </Frame>
  )
}
