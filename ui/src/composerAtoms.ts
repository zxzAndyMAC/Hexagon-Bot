// 票 10：输入框原子块。正文仍是 @角色 / #路径 纯文本——parse_tokens 与
// dispatch 的分词口径不变，块只决定退格/删除一次去掉哪一段。
//
// 收束规则：词后已有空白才升成块（弹层插入会补一个尾随空格；手打路径
// 打完再空格）。文末还在打的词不是块，否则补全查询没法逐字删。
// 被否决的替代：一匹配花名册就吞掉——「产品策划」是「产品策划助手」的
// 前缀，还没打完就被整块吃掉。
//
// 目录不展开：路径块的值就是这一条路径（目录保留尾 /）。没有「把子文件
// 写进正文」的入口；票 12 拖入必须走 pathTokenText，得到同一种 #路径。

export type AtomKind = 'mention' | 'path'

export type Atom = {
  kind: AtomKind
  /** 不含 @ / # 前缀。目录带尾 /。 */
  value: string
  start: number
  end: number
}

export function findAtoms(text: string, roles: ReadonlySet<string>): Atom[] {
  const out: Atom[] = []
  const re = /(^|\s)([@#]\S+)/g
  let m: RegExpExecArray | null
  while ((m = re.exec(text))) {
    const raw = m[2]
    const start = m.index + m[1].length
    const end = start + raw.length
    const after = text[end]
    if (after === undefined || !/\s/.test(after)) continue
    if (raw.startsWith('@')) {
      const role = raw.slice(1)
      if (!role || !roles.has(role)) continue
      out.push({ kind: 'mention', value: role, start, end })
    } else {
      const path = raw.slice(1)
      if (!path) continue
      out.push({ kind: 'path', value: path, start, end })
    }
  }
  return out
}

function trailLen(text: string, end: number): number {
  return end < text.length && /\s/.test(text[end]) ? 1 : 0
}

/**
 * 退格/删除一次去掉整块，含紧随的一个空白，不留角色名或路径残字。
 * 光标在块内，或在块后那个收束空白上，都算打中这块。
 * 选区只盖住残段时扩成整块。没打中任何块则返回 null，交给 textarea。
 */
export function atomicDeletion(
  text: string,
  selStart: number,
  selEnd: number,
  key: 'Backspace' | 'Delete',
  atoms: readonly Atom[],
): { text: string; caret: number } | null {
  if (atoms.length === 0) return null
  let from = Math.min(selStart, selEnd)
  let to = Math.max(selStart, selEnd)
  const span = (a: Atom) => ({ from: a.start, to: a.end + trailLen(text, a.end) })

  if (from === to) {
    const caret = from
    const hit = atoms.find((a) => {
      const s = span(a)
      if (key === 'Backspace') return caret > s.from && caret <= s.to
      return caret >= s.from && caret < s.to
    })
    if (!hit) return null
    const s = span(hit)
    from = s.from
    to = s.to
  } else {
    let touched = false
    for (const a of atoms) {
      const s = span(a)
      if (from < s.to && to > s.from) {
        touched = true
        from = Math.min(from, s.from)
        to = Math.max(to, s.to)
      }
    }
    if (!touched) return null
  }
  return { text: text.slice(0, from) + text.slice(to), caret: from }
}

/** 票 12 拖入与手打 / # 弹层共用。只返回这一条路径，不拼接子文件。 */
export function pathTokenText(path: string): string {
  return `#${path.trim().replace(/^#+/, '')}`
}

export function mentionTokenText(role: string): string {
  return `@${role.trim().replace(/^@+/, '')}`
}
