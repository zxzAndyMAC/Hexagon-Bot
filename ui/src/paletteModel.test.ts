// ui-audit 票 09：palette 匹配/排序 + 最近使用。
import { describe, expect, it } from 'vitest'
import { filterPaletteItems, pushRecent } from './paletteModel'

const items = [
  { id: 'c-stamp', label: '盖章' },
  { id: 'c-rewind', label: '退回上一阶段', keywords: ['rew', 'rewind', 'back'] },
  { id: 'c-skip', label: '跳过当前阶段', keywords: ['skip', 'pass'] },
  { id: 'n-timeline', label: 'Timeline' },
]

describe('filterPaletteItems', () => {
  it('keywords 参与匹配：rew 命中「退回上一阶段」', () => {
    const out = filterPaletteItems(items, 'rew', [])
    expect(out.map((i) => i.id)).toEqual(['c-rewind'])
  })

  it('label 匹配照常（中文标签直查）', () => {
    const out = filterPaletteItems(items, '跳过', [])
    expect(out.map((i) => i.id)).toEqual(['c-skip'])
  })

  it('大小写不敏感', () => {
    expect(filterPaletteItems(items, 'REW', []).map((i) => i.id)).toEqual(['c-rewind'])
    expect(filterPaletteItems(items, 'time', []).map((i) => i.id)).toEqual(['n-timeline'])
  })

  it('空查询：最近使用置顶，同 rank 保声明序', () => {
    const out = filterPaletteItems(items, '', ['c-skip', 'c-stamp'])
    expect(out.map((i) => i.id)).toEqual(['c-skip', 'c-stamp', 'c-rewind', 'n-timeline'])
  })

  it('非空查询不置顶 recents（按匹配定）', () => {
    // stamp 虽是 recent，但不匹配 rew → 不出现
    const out = filterPaletteItems(items, 'rew', ['c-stamp'])
    expect(out.map((i) => i.id)).toEqual(['c-rewind'])
  })
})

describe('pushRecent', () => {
  it('新项置顶、去重、截断 cap', () => {
    let r: string[] = []
    r = pushRecent(r, 'a')
    r = pushRecent(r, 'b')
    r = pushRecent(r, 'a') // 重复使用回顶
    expect(r).toEqual(['a', 'b'])
    for (let i = 0; i < 10; i++) r = pushRecent(r, `x${i}`)
    expect(r).toHaveLength(8)
    expect(r[0]).toBe('x9')
  })
})
