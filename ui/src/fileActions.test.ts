import { describe, expect, it } from 'vitest'
import { absolutePath, artifactRepoPath, projectRelPath } from './fileActions'

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

// #32（owner-followups-1015）：表面上的路径（相对或绝对）→ 项目相对；根外 / 无根的绝对路径 → null（不出路径类项）。
describe('projectRelPath', () => {
  it('根内绝对路径 → 相对；根本身 → 空串', () => {
    expect(projectRelPath('/Users/me/todo/src/main.rs', '/Users/me/todo')).toBe('src/main.rs')
    expect(projectRelPath('/Users/me/todo', '/Users/me/todo/')).toBe('')
    expect(projectRelPath('C:\\work\\todo\\src\\a.rs', 'C:\\work\\todo')).toBe('src/a.rs')
  })
  it('根外 / 前缀相同的兄弟目录 / 无根 → null', () => {
    expect(projectRelPath('/Users/me/other/a.md', '/Users/me/todo')).toBeNull()
    expect(projectRelPath('/Users/me/todo2/a.md', '/Users/me/todo')).toBeNull()
    expect(projectRelPath('/etc/passwd', null)).toBeNull()
  })
  it('相对路径原样（去 ./），含 .. 段 → null', () => {
    expect(projectRelPath('./docs/plan.md', '/r')).toBe('docs/plan.md')
    expect(projectRelPath('docs/plan.md', null)).toBe('docs/plan.md')
    expect(projectRelPath('../secret', '/r')).toBeNull()
  })
})

// #32 修复：产物登记路径是 .hexagon/ 内相对，盘上仓内路径 = .hexagon/<path>。
describe('artifactRepoPath', () => {
  it('登记路径补 .hexagon/；已带前缀或绝对形原样', () => {
    expect(artifactRepoPath('specs/prd.md')).toBe('.hexagon/specs/prd.md')
    expect(artifactRepoPath('.hexagon/specs/prd.md')).toBe('.hexagon/specs/prd.md')
    expect(artifactRepoPath('/abs/x.md')).toBe('/abs/x.md')
  })
})
