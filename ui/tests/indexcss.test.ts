import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

// 回归：Tauri/WKWebView 对微溢出的 document 给橡皮筋滚动——「整个界面
// 可上下左右拖动」那次 bug 就是根上没锁。文档级 overflow:hidden +
// overscroll-behavior:none 必须留在 html,body,#root 规则里。
// 注：此测试故意放 tests/（tsconfig.app 只 include src/，node API 不进 app 类型面）。
describe('index.css 文档锁', () => {
  const css = readFileSync(resolve(process.cwd(), 'src/index.css'), 'utf8')
  const rootRule = css.match(/html,\s*body,\s*#root\s*\{([^}]*)\}/)?.[1] ?? ''

  it('html/body/#root 锁 overflow:hidden + overscroll-behavior:none', () => {
    expect(rootRule).toMatch(/overflow:\s*hidden/)
    expect(rootRule).toMatch(/overscroll-behavior:\s*none/)
  })
})
