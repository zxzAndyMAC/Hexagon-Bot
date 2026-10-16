// gap D4 / D5（ui-redesign 16B，settings#skills / settings#mcp 列表底 ld-foot）：
// 技能筛选占位带总数「筛选 N 个技能」+「安装…」单按钮开菜单（文件夹 / ZIP，脚注写明安装 ≠ 授权；
// 浏览器预览无本机文件对话框→菜单项禁用写原因）；MCP 列表底「市场」常驻（原先只在桌面壳出现）并调 open_mcp_market。
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../../i18n'
import { api } from '../../api'
import type { SkillRow } from '../../gen/SkillRow'
import { SkillsSection } from './SkillsSection'
import { McpSection } from './McpSection'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const skill = (name: string): SkillRow => ({ name, description: `${name} desc`, origin: 'global', enabled: true, legacy_experience_blocks: 0 } as SkillRow)

let el: HTMLDivElement
let root: Root
beforeEach(async () => {
  await i18n.changeLanguage('en')
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
})
afterEach(async () => { await act(async () => root.unmount()); el.remove(); document.body.innerHTML = ''; vi.restoreAllMocks() })

describe('settings#skills 列表底（gap D4）', () => {
  it('筛选占位带技能总数', async () => {
    vi.spyOn(api, 'listSkills').mockResolvedValue([skill('alpha'), skill('beta'), skill('gamma')])
    await act(async () => root.render(<SkillsSection />))
    const search = el.querySelector<HTMLInputElement>('input[placeholder]')!
    expect(search.placeholder).toBe(i18n.t('skills.filterN', { count: 3 }))
    expect(search.placeholder).toContain('3')
  })

  it('「安装…」开菜单：文件夹 / ZIP 两项 + 安装≠授权脚注；浏览器预览下两项禁用并写原因', async () => {
    vi.spyOn(api, 'listSkills').mockResolvedValue([skill('alpha')])
    await act(async () => root.render(<SkillsSection />))
    const trigger = el.querySelector<HTMLButtonElement>('[data-skill-install]')!
    expect(trigger.textContent).toBe(i18n.t('skills.install'))
    await act(async () => {
      trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    })
    const menu = document.querySelector('[role="menu"]')!
    expect(menu).not.toBeNull()
    const items = Array.from(menu.querySelectorAll('[role="menuitem"]'))
    expect(items).toHaveLength(2)
    expect(items[0].textContent).toContain(i18n.t('skills.installDir'))
    expect(items[1].textContent).toContain(i18n.t('skills.installZip'))
    for (const item of items) {
      expect(item.getAttribute('aria-disabled')).toBe('true')
      expect(item.textContent).toContain(i18n.t('skills.installDesktopOnly'))
    }
    expect(menu.textContent).toContain(i18n.t('skills.installNote'))
  })
})

describe('settings#mcp 列表底（gap D5）', () => {
  it('「市场」按钮常驻并调 openMcpMarket；title 写明去向', async () => {
    vi.spyOn(api, 'mcpEntries').mockResolvedValue([])
    vi.spyOn(api, 'mcpServices').mockResolvedValue([])
    const open = vi.spyOn(api, 'openMcpMarket').mockResolvedValue(undefined)
    await act(async () => root.render(<McpSection projectless={false} />))
    const btn = el.querySelector<HTMLButtonElement>('[data-mcp-market]')!
    expect(btn.textContent).toBe(i18n.t('mcp.market'))
    expect(btn.title).toBe(i18n.t('mcp.marketTip'))
    await act(async () => btn.click())
    expect(open).toHaveBeenCalledTimes(1)
  })
})
