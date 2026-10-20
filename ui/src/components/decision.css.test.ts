/// <reference types="node" />
// owner-followups-1015 #26 回归（2026-10-20 负责人截图，不可逆待决卡「推送到远端」）：
// 坞内卡 `.dock-card` 自己没有圆角，危险态的红 30% 内环画在它的直角盒上；外壳 `.dock` 圆角 16、不裁切，
// 于是红框四角是直角、从坞的圆角外戳出来，与坞的环线错开像隔了一道缝。
// 这里守住几何：坞内卡继承外壳圆角（同一 16 常量，不另写数），危险环线画在卡本身；独立形 `dc` 的外环跟 `--r-panel`。
// vitest 关了 CSS 处理（`?raw` 读出空串），直接按 ui 根读源文件；只断言规则，不测像素（像素归 test:visual）。
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const css = readFileSync(resolve(process.cwd(), 'src/components/decision.css'), 'utf8')

/** 选择器恰好为 `selector` 的规则体（全部同名规则拼起来）。 */
function rule(selector: string): string {
  const esc = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const re = new RegExp(`(?:^|\\})\\s*${esc}\\s*\\{([^}]*)\\}`, 'g')
  return [...css.replace(/\/\*[\s\S]*?\*\//g, '').matchAll(re)].map((m) => m[1]).join(';')
}

const RED_RING = /0 0 0 1px color-mix\(in oklch, var\(--red\) 30%, transparent\)/

describe('危险态待决卡描边（#26）', () => {
  it('坞内卡继承外壳圆角，红环不再画在直角盒上', () => {
    expect(rule('.dock')).toMatch(/border-radius:\s*16px/)
    expect(rule('.dock .dock-card')).toMatch(/border-radius:\s*inherit/)
  })

  it('坞内危险卡：红 30% 内环画在卡本身，不在外层 wrapper', () => {
    const body = rule('.dock-card[data-danger]')
    expect(body).toMatch(/box-shadow:\s*inset 0 0 0 1px/)
    expect(body).toMatch(RED_RING)
  })

  it('独立形 dc 危险卡：外环跟 --r-panel 圆角，自身裁切不碰外环', () => {
    expect(rule('.dc')).toMatch(/border-radius:\s*var\(--r-panel\)/)
    const body = rule('.dc[data-danger]')
    expect(body).toMatch(RED_RING)
    expect(body).not.toMatch(/inset/)
  })
})
