// 全局右键拦截（components A16b「系统菜单」，owner-followups-1015 #05）：
// WebView 自带菜单（重新载入 / 检查元素）不给负责人看；输入区与 Monaco 保留系统 / 编辑器菜单。
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { installContextMenuGuard, keepsNativeMenu } from './contextMenuGuard'

let uninstall: () => void
let host: HTMLDivElement

const rightClick = (target: Element) => {
  const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
  target.dispatchEvent(ev)
  return ev.defaultPrevented
}

beforeEach(() => {
  uninstall = installContextMenuGuard(window)
  host = document.createElement('div')
  host.innerHTML = `
    <p data-plain>text <span data-inner>inner</span></p>
    <input data-input />
    <textarea data-textarea></textarea>
    <div contenteditable="true" data-ce><b data-ce-inner>bold</b></div>
    <div contenteditable="false" data-ce-off>off</div>
    <div class="monaco-editor"><div data-monaco-line>code</div></div>`
  document.body.appendChild(host)
})

afterEach(() => {
  uninstall()
  host.remove()
})

describe('全局右键拦截', () => {
  it('普通表面右键：阻止 WebView 自带菜单', () => {
    expect(rightClick(host.querySelector('[data-plain]')!)).toBe(true)
    expect(rightClick(host.querySelector('[data-inner]')!)).toBe(true)
    expect(rightClick(host.querySelector('[data-ce-off]')!)).toBe(true)
  })

  it('输入框、多行输入、contenteditable 保留系统菜单（拼写、粘贴）', () => {
    expect(rightClick(host.querySelector('[data-input]')!)).toBe(false)
    expect(rightClick(host.querySelector('[data-textarea]')!)).toBe(false)
    expect(rightClick(host.querySelector('[data-ce-inner]')!)).toBe(false)
  })

  it('Monaco 区域保留编辑器自己的菜单', () => {
    expect(rightClick(host.querySelector('[data-monaco-line]')!)).toBe(false)
  })

  it('卸载后不再拦截', () => {
    uninstall()
    expect(rightClick(host.querySelector('[data-plain]')!)).toBe(false)
    uninstall = installContextMenuGuard(window)
  })

  it('keepsNativeMenu 对非元素目标返回 false', () => {
    expect(keepsNativeMenu(null)).toBe(false)
    expect(keepsNativeMenu(window)).toBe(false)
  })
})
