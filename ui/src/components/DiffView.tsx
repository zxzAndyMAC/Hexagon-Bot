import { useMemo } from 'react'
import type { DiffOp } from '../diff'
import { CodeBlock } from './Md'

const LINE_STYLE: Record<DiffOp['type'], React.CSSProperties> = {
  ins: { background: 'var(--ok-soft)', color: 'var(--ok)' },
  del: { background: 'var(--err-soft)', color: 'var(--err)' },
  eq: {},
}

const SIGN: Record<DiffOp['type'], string> = { ins: '+', del: '-', eq: ' ' }

/** 统一红绿 diff 视图（无 Monaco：op 序列行渲染）。 */
export function DiffView({ ops }: { ops: DiffOp[] }) {
  const rows = useMemo(
    () =>
      ops.map((o, i) => (
        <div key={i} style={{ display: 'flex', ...LINE_STYLE[o.type] }}>
          <span className="dim3" style={{ width: 14, flexShrink: 0, userSelect: 'none', textAlign: 'center' }}>
            {SIGN[o.type]}
          </span>
          <span style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>{o.text}</span>
        </div>
      )),
    [ops],
  )
  return (
    <div className="mono" style={{ flex: 1, overflowY: 'auto', fontSize: 12, lineHeight: 1.55, padding: '8px 0' }}>
      {rows}
    </div>
  )
}

/** 只读代码视图（Shiki 高亮）。 */
export function CodeView({ text, lang }: { text: string; lang?: string }) {
  return (
    <div className="mono" style={{ flex: 1, overflowY: 'auto', fontSize: 12, lineHeight: 1.55, padding: '8px 14px' }}>
      <CodeBlock code={text} lang={lang} />
    </div>
  )
}
