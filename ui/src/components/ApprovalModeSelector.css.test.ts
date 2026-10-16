/// <reference types="node" />
// 票 16a 回归（composer#broad）：ApprovalModeSelector.css 是未分层样式，任何 margin 声明都会压过
// 组件上的 Tailwind 间距类（`broad-list mt-3`），说明与勾选行贴在一起。这里守住规则里不再写 margin。
// vitest 关了 CSS 处理（`?raw` 读出空串），直接按 ui 根读源文件。
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { expect, it } from 'vitest'

it('.broad-list 不声明 margin，间距交给组件上的 mt-3', () => {
  const css = readFileSync(resolve(process.cwd(), 'src/components/ApprovalModeSelector.css'), 'utf8')
  const rule = css.match(/\.broad-list\s*\{([^}]*)\}/)?.[1] ?? ''
  expect(rule).not.toBe('')
  expect(rule).not.toMatch(/\bmargin\b/)
})
