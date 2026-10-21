import { describe, expect, it } from 'vitest'
import { absolutePath } from './fileActions'

// 票 29（owner-followups-1015）：画布页签「复制路径」写绝对路径——项目根 + 项目相对路径。
describe('absolutePath', () => {
  it('POSIX 根：用 / 拼接，根末尾的 / 不重复', () => {
    expect(absolutePath('/Users/me/todo', 'src/main.rs')).toBe('/Users/me/todo/src/main.rs')
    expect(absolutePath('/Users/me/todo/', 'src/main.rs')).toBe('/Users/me/todo/src/main.rs')
  })

  it('Windows 根：分隔符跟根走，相对路径的 / 换成反斜杠', () => {
    expect(absolutePath('C:\\work\\todo', 'src/main.rs')).toBe('C:\\work\\todo\\src\\main.rs')
    expect(absolutePath('C:\\work\\todo\\', 'a.md')).toBe('C:\\work\\todo\\a.md')
  })

  it('没有项目根：原样返回相对路径', () => {
    expect(absolutePath(null, 'src/main.rs')).toBe('src/main.rs')
  })
})
