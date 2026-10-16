// 图例 / ctip 排序（票 16B 原型逐节核对）：按 config 声明序，不倒排。
// usage#detail 原型图例与提示都是「工具输出 / 输出 / 输入」，UsageView 就按这个顺序声明。
import { describe, expect, it } from 'vitest'
import type { ChartConfig } from './chart'
import { byConfigOrder } from './chartOrder'

const config: ChartConfig = {
  tool: { label: '工具输出', color: 'var(--chart-3)' },
  output: { label: '输出', color: 'var(--chart-2)' },
  input: { label: '输入', color: 'var(--chart-1)' },
}

describe('byConfigOrder', () => {
  it('Recharts 给的任意顺序都排回声明序，并滤掉 type=none', () => {
    const payload = [
      { dataKey: 'input' }, { dataKey: 'ghost', type: 'none' }, { dataKey: 'tool' }, { dataKey: 'output' },
    ]
    expect(byConfigOrder(payload, config).map((p) => p.dataKey)).toEqual(['tool', 'output', 'input'])
  })

  it('图例 payload 只有 value 时也按声明序', () => {
    const payload = [{ value: 'output' }, { value: 'input' }, { value: 'tool' }]
    expect(byConfigOrder(payload, config).map((p) => p.value)).toEqual(['tool', 'output', 'input'])
  })
})
