// @vitest-environment node
// check-tokens 对比度断言的单测（owner-followups-1015 票 09）：既证明现行 index.css 达标，
// 也故意喂一个不达标值确认断言会红——否则「解析失败当通过」这类静默失效测不出来。
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { checkContrast, contrastRatio, oklchToSrgb, parseOklch, CONTRAST_TABLE } from './contrast.mjs'

const css = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')

/** 把某主题块里某 token 的声明值换掉（只换第一处匹配，块用选择器开头定位）。 */
function patch(src, selector, token, value) {
  const at = src.indexOf(selector)
  const end = src.indexOf('}', at)
  const block = src.slice(at, end)
  const next = block.replace(new RegExp(`(${token}\\s*:\\s*)[^;]+;`), `$1${value};`)
  if (next === block) throw new Error(`${token} not in ${selector}`)
  return src.slice(0, at) + next + src.slice(end)
}

describe('对比度计算', () => {
  it('黑白 21:1，同色 1:1', () => {
    const white = oklchToSrgb(parseOklch('oklch(100% 0 0)'))
    const black = oklchToSrgb(parseOklch('oklch(0% 0 0)'))
    expect(contrastRatio(white, black)).toBeCloseTo(21, 1)
    expect(contrastRatio(white, white)).toBeCloseTo(1, 5)
  })

  it('解析 alpha 与 0–1 小数亮度', () => {
    const p = parseOklch('oklch(66.6% .18 21.433 / .14)')
    expect(p.l).toBeCloseTo(0.666, 6)
    expect([p.c, p.h, p.a]).toEqual([0.18, 21.433, 0.14])
    expect(parseOklch('oklch(50% 0 0 / 40%)')?.a).toBeCloseTo(0.4, 6)
    expect(parseOklch('oklch(.5 0 0)')?.l).toBe(0.5)
    expect(parseOklch('#fff')).toBeNull()
  })
})

describe('index.css 对比度断言表', () => {
  it('深浅两套全部达标', () => {
    expect(checkContrast(css)).toEqual([])
  })

  it('表覆盖辅助文字、主按钮白字与四个语义 -ink', () => {
    const fgs = new Set(CONTRAST_TABLE.map((r) => `${r.fg}@${r.bg}`))
    expect(fgs.has('--ink-3@--surface')).toBe(true)
    expect(fgs.has('white@--accent-strong')).toBe(true)
    for (const c of ['green', 'orange', 'red', 'violet']) expect(fgs.has(`--${c}-ink@--${c}-tint`)).toBe(true)
  })

  it('把浅色 --ink-3 改回旧值（L 69.5%）即报红', () => {
    const bad = checkContrast(patch(css, ':root,', '--ink-3', 'oklch(69.5% .009 264.505)'))
    expect(bad.some((b) => b.startsWith('light: --ink-3 on --surface'))).toBe(true)
  })

  it('把深色主按钮底改回 --accent 即报红', () => {
    const bad = checkContrast(patch(css, "[data-theme='dark'],", '--accent-strong', 'var(--accent)'))
    expect(bad).toContainEqual(expect.stringMatching(/^dark: white on --accent-strong = 2\.\d\d:1/))
  })

  it('表里写了未定义的 token 直接抛错，不当通过', () => {
    expect(() => checkContrast(css, [{ fg: '--nope', bg: '--surface', min: 4.5 }])).toThrow(/--nope/)
  })
})
