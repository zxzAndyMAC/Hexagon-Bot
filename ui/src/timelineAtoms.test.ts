import { expect, it } from 'vitest'
import { splitAtoms } from './timelineAtoms'

const roles = new Set(['前端', '后端'])
const kinds = (nodes: ReturnType<typeof splitAtoms>) => nodes.map((n) => n.type === 'text' ? `t:${n.value}` : `${(n.data!.hProperties as { className: string[] }).className.join('.')}:${n.children![0].value}`)

it('owner text turns roster mentions and path tokens into atoms (C3)', () => {
  expect(kinds(splitAtoms('按 #.hexagon/ui/todo-list.md 做，@前端 负责。', { roles, owner: true })))
    .toEqual(['t:按 ', 'tl-atom.p:#.hexagon/ui/todo-list.md', 't: 做，', 'tl-atom.m:@前端', 't: 负责。'])
})

it('agent text only colors mentions; unknown names and numbered tags stay text (C1)', () => {
  expect(kinds(splitAtoms('@前端，看 #12 和 @路人', { roles, owner: false }))).toEqual(['tl-mention:@前端', 't:，看 #12 和 @路人'])
})

it('a path atom is a button carrying the path for opening the file', () => {
  const node = splitAtoms('#src/a.ts', { roles, owner: true })[0]
  expect(node.data!.hName).toBe('button')
  expect((node.data!.hProperties as Record<string, unknown>)['data-atom-path']).toBe('src/a.ts')
})
