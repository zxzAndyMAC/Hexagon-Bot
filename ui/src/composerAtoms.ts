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
//
// 拖放载荷用自定义 MIME，不用 Files、也不用 text/plain 当准入。
// Files 会撞上图片附件通道，浏览器还可能把拖放画成「移动文件」。
// text/plain 谁都能带，外部拖一段字进来不该变成路径块。
// effectAllowed=copy：只复制路径文本，不改磁盘上的文件（票 12）。

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

/** 文件树 → 输入框。见文件头：不走 Files / text/plain。 */
export const TREE_DRAG_MIME = 'application/x-hexagon-path'

function oneRepoPath(path: string, kind: 'file' | 'dir'): string | null {
  const raw = path.trim().replace(/\\/g, '/')
  if (!raw || raw.startsWith('/') || /\s/.test(raw)) return null
  const parts = raw.split('/').filter((p) => p.length > 0)
  if (parts.length === 0 || parts.some((p) => p === '.' || p === '..')) return null
  const joined = parts.join('/')
  return kind === 'dir' ? `${joined}/` : joined
}

/** 文件或目录 → 一条载荷。目录补尾 /，与 repo_paths 的目录形一致。链接返回 null。 */
export function treeDragPayload(entry: { path: string; kind: string }): { payload: string; token: string } | null {
  if (entry.kind !== 'file' && entry.kind !== 'dir') return null
  const path = oneRepoPath(entry.path, entry.kind)
  if (!path) return null
  return { payload: JSON.stringify({ path, kind: entry.kind }), token: pathTokenText(path) }
}

/** 只接受规范化后的单条路径。对不上（缺尾 /、夹了子文件、越出仓根）就丢掉。 */
export function pathFromTreeDrag(raw: string): string | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return null
  }
  if (!parsed || typeof parsed !== 'object') return null
  const rec = parsed as { path?: unknown; kind?: unknown }
  if (typeof rec.path !== 'string' || (rec.kind !== 'file' && rec.kind !== 'dir')) return null
  const path = oneRepoPath(rec.path, rec.kind)
  if (!path || path !== rec.path) return null
  return path
}

/**
 * 在光标处插入一条路径块，并补上收束空白，使退格一次删整块。
 * 不读文件、不展开目录——path 是什么就只插入这一条。
 */
export function insertPathToken(text: string, caret: number, path: string): { text: string; caret: number } | null {
  const token = pathTokenText(path)
  if (token === '#') return null
  const at = Math.max(0, Math.min(caret, text.length))
  const before = text.slice(0, at)
  const after = text.slice(at)
  const lead = before.length > 0 && !/\s$/.test(before) ? ' ' : ''
  const trail = after.length === 0 || !/^\s/.test(after) ? ' ' : ''
  const piece = `${lead}${token}${trail}`
  return { text: before + piece + after, caret: before.length + piece.length }
}

export function mentionTokenText(role: string): string {
  return `@${role.trim().replace(/^@+/, '')}`
}
