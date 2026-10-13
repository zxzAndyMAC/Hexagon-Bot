// 时间线正文里的点名与路径（ui-redesign 票 05，components C1「点名」、C2/C3「原子块」）。
// remark 插件：只改写 mdast 文本节点，代码、行内代码、链接里的 @ / # 不动。
// 负责人消息：@角色 → 点名块（accent），#路径 → 路径块（violet，可点打开文件）。
// Agent 消息：只把 @角色 染成 mention 色，不升块（C1）。
// 口径与输入条一致（composerAtoms）：点名必须命中花名册；路径必须像路径（含 / 或 .），
// 否则「#12」这类编号会被误认成路径块。与输入条不同的是这里不要求词尾空白——
// 已发出的消息没有「还在打字」的半截词，「@前端，」这类中文标点紧贴也要认。
type MdNode = { type: string; value?: string; children?: MdNode[]; data?: Record<string, unknown> }

const SKIP = new Set(['code', 'inlineCode', 'link', 'linkReference', 'html'])
const TOKEN = /[@#][^\s@#]+/g
const PATH_SHAPE = /^[\w.\-/]*[\w/]$/

export type AtomOptions = { roles: ReadonlySet<string>; owner: boolean }

function longestRole(raw: string, roles: ReadonlySet<string>): string | null {
  let best: string | null = null
  for (const r of roles) if (raw.startsWith(r) && (!best || r.length > best.length)) best = r
  return best
}

function atomNode(kind: 'm' | 'p' | 'mention', text: string, path?: string): MdNode {
  const hProperties: Record<string, unknown> = kind === 'mention'
    ? { className: ['tl-mention'] }
    : { className: ['tl-atom', kind], ...(path ? { type: 'button', 'data-atom-path': path } : {}) }
  return { type: 'tlAtom', data: { hName: path ? 'button' : 'span', hProperties }, children: [{ type: 'text', value: text }] }
}

export function splitAtoms(value: string, { roles, owner }: AtomOptions): MdNode[] {
  const out: MdNode[] = []
  let last = 0
  for (const m of value.matchAll(TOKEN)) {
    const at = m.index ?? 0
    if (at > 0 && !/\s/.test(value[at - 1]) && !/[（(「“"']/.test(value[at - 1])) continue
    const raw = m[0]
    let token: string | null = null
    let node: MdNode | null = null
    if (raw[0] === '@') {
      const role = longestRole(raw.slice(1), roles)
      if (role) { token = `@${role}`; node = atomNode(owner ? 'm' : 'mention', token) }
    } else if (owner) {
      const path = raw.slice(1).replace(/[，。,;；:：)）」”"']+$/, '')
      if (path && PATH_SHAPE.test(path) && /[/.]/.test(path)) { token = `#${path}`; node = atomNode('p', token, path) }
    }
    if (!token || !node) continue
    if (at > last) out.push({ type: 'text', value: value.slice(last, at) })
    out.push(node)
    last = at + token.length
  }
  if (!out.length) return [{ type: 'text', value }]
  if (last < value.length) out.push({ type: 'text', value: value.slice(last) })
  return out
}

function walk(node: MdNode, opts: AtomOptions) {
  if (!node.children || SKIP.has(node.type)) return
  const next: MdNode[] = []
  for (const child of node.children) {
    if (child.type === 'text' && typeof child.value === 'string') next.push(...splitAtoms(child.value, opts))
    else { walk(child, opts); next.push(child) }
  }
  node.children = next
}

/** react-markdown 的 remarkPlugins 项：`[remarkAtoms, { roles, owner }]`。 */
export function remarkAtoms(opts: AtomOptions) {
  return (tree: MdNode) => { if (opts.roles.size || opts.owner) walk(tree, opts) }
}
