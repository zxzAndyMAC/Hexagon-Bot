// 票 16c（非英文冒烟）：主路径上的关键值/主操作文字不得被省略号吃掉。
// happy-dom 不排版，量不了 scrollWidth；这里钉住样式规则本身，实测走 Playwright 冒烟（shots/16c）。
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

// css?raw 在 vitest 里被 css 处理吞成空串，直接读源文件。
const timelineCss = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'components/timeline.css'), 'utf8')

const rule = (css: string, selector: string): string => {
  const at = css.indexOf(`${selector} {`)
  expect(at, `rule ${selector} exists`).toBeGreaterThanOrEqual(0)
  return css.slice(at, css.indexOf('}', at))
}

describe('no truncation on the golden path', () => {
  it('return-summary KPI values wrap instead of ellipsizing (fr/ja/en “2 passed · 1 sent back”)', () => {
    const r = rule(timelineCss, '.tl-rs-cell > b')
    expect(r).not.toMatch(/text-overflow:\s*ellipsis/)
    expect(r).not.toMatch(/white-space:\s*nowrap/)
  })
})
