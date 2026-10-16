// 票 16a 回归：钉顶归来摘要超高时必须「卡内滚动」（components C12），不能由外壳裁掉卡片下沿。
// jsdom 不排版，这里只能钉住样式契约本身：外壳限高不滚，直接子元素（卡片）自己缩并滚动。
import { describe, expect, it } from 'vitest'

// vitest 不处理 CSS（import / ?raw 都拿到空串），也没装 @types/node：经变量动态导入 node:fs 读源文件
// （jsdom 环境下 import.meta.url 不是 file: 协议，按 vitest 的 cwd = ui/ 拼路径）
const [fsMod, procMod] = ['node:fs', 'node:process']
const { readFileSync } = await import(/* @vite-ignore */ fsMod) as { readFileSync: (p: string, enc: string) => string }
const { cwd } = await import(/* @vite-ignore */ procMod) as { cwd: () => string }
const css = readFileSync(`${cwd()}/src/components/timeline.css`, 'utf8')

function rule(selector: string): string {
  const esc = selector.replace(/[.*+?^${}()|[\]\\>]/g, '\\$&')
  const m = css.match(new RegExp(`(^|\\n)${esc}\\s*\\{([^}]*)\\}`))
  return m ? m[2] : ''
}

describe('钉顶归来摘要', () => {
  it('外壳只限高，不在外壳上滚动（否则卡片下沿与圆角被裁）', () => {
    const shell = rule('.tl-pin')
    expect(shell).toMatch(/max-height:\s*55%/)
    expect(shell).toMatch(/flex-direction:\s*column/)
    expect(shell).not.toMatch(/overflow-y:\s*auto/)
  })
  it('卡片自身可缩并在卡内滚动', () => {
    const card = rule('.tl-pin > *')
    expect(card).toMatch(/min-height:\s*0/)
    expect(card).toMatch(/overflow-y:\s*auto/)
  })
})
