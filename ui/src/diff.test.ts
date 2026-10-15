// 统一 diff 行号与 +N −N（ui-redesign 票 09，components D13 / D15，canvas#artifact）。
import { expect, it } from 'vitest'
import { diffLines, diffStats, numberDiff } from './diff'

it('删除行用旧行号、新增与等同行用新行号，并带两侧下标', () => {
  const ops = diffLines(['a', 'b', 'c'], ['a', 'B', 'x', 'c'])
  expect(numberDiff(ops).map((o) => [o.type, o.line, o.a, o.b])).toEqual([
    ['eq', 1, 0, 0],
    ['del', 2, 1, null],
    ['ins', 2, null, 1],
    ['ins', 3, null, 2],
    ['eq', 4, 2, 3],
  ])
})

it('统计新增与删除行数', () => {
  expect(diffStats(diffLines(['a', 'b'], ['a', 'c', 'd']))).toEqual({ add: 2, del: 1 })
  expect(diffStats(diffLines(['a'], ['a']))).toEqual({ add: 0, del: 0 })
})
