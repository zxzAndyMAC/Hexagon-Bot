// owner-followups-1015 #10（规范 components E2「列表行不嵌开关」、A10「命中区」，2026-10-17 负责人裁决 followups 第 8 项）：
// 技能 / MCP 列表行（role=option）里曾嵌 A10 开关——读屏读不出（axe nested-interactive），开关 30×18 也点不准。
// 现在行只表状态：停用 = 状态点 off + neutral 胶囊「已停用」，启用不出东西；开关只在详情（技能 = 详情头部，
// MCP = 详情表单已有开关）。这里钉住「option 内无可交互后代」与停用胶囊，以及技能详情头部开关真能切换。
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../../i18n'
import { api } from '../../api'
import type { SkillRow } from '../../gen/SkillRow'
import type { McpEntryRow } from '../../gen/McpEntryRow'
import { SkillsSection } from './SkillsSection'
import { McpSection } from './McpSection'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const INTERACTIVE = 'button, input, select, textarea, a[href], [role="switch"], [role="checkbox"], [tabindex]'

const skill = (name: string, enabled = true): SkillRow =>
  ({ name, description: `${name} desc`, origin: 'global', enabled, legacy_experience_blocks: 0 } as SkillRow)
const mcp = (name: string, disabled = false): McpEntryRow => ({
  name, command: 'npx', args: [], env: {}, cwd: null, disabled, transport: 'stdio', url: null, headers: {}, origin: 'global',
})

let el: HTMLDivElement
let root: Root
beforeEach(async () => {
  await i18n.changeLanguage('en')
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
})
afterEach(async () => { await act(async () => root.unmount()); el.remove(); document.body.innerHTML = ''; vi.restoreAllMocks() })

const options = () => [...el.querySelectorAll<HTMLElement>('[role="option"]')]
const row = (key: string) => el.querySelector<HTMLElement>(`[role="option"][data-key="${key}"]`)!
const chip = (o: HTMLElement) => o.querySelector('[data-disabled-chip]')

describe('settings#skills 列表行（#10）', () => {
  it('option 内无可交互后代；停用行 = 状态点 off + 「已停用」胶囊，不整行半透明；启用行不出状态', async () => {
    vi.spyOn(api, 'listSkills').mockResolvedValue([skill('alpha'), skill('beta', false)])
    await act(async () => root.render(<SkillsSection />))
    expect(options()).toHaveLength(2)
    for (const o of options()) expect(o.querySelectorAll(INTERACTIVE)).toHaveLength(0)
    const off = row('beta')
    expect(chip(off)?.textContent).toBe(i18n.t('settingsUi.disabledChip'))
    expect(off.querySelector('[data-slot="dot"]')?.getAttribute('data-tone')).toBe('off')
    expect(off.className).not.toMatch(/opacity/)
    const on = row('alpha')
    expect(chip(on)).toBeNull()
    expect(on.querySelector('[data-slot="dot"]')).toBeNull()
  })

  it('详情头部开关切换启用：调 setSkillMuted，列表行随之出 / 收「已停用」', async () => {
    vi.spyOn(api, 'listSkills').mockResolvedValue([skill('alpha')])
    vi.spyOn(api, 'skillFiles').mockResolvedValue(['SKILL.md'])
    vi.spyOn(api, 'readSkillFile').mockResolvedValue('# alpha\n')
    const mute = vi.spyOn(api, 'setSkillMuted').mockResolvedValue(undefined as never)
    await act(async () => root.render(<SkillsSection />))
    await act(async () => row('alpha').click())
    const sw = el.querySelector<HTMLElement>('[data-skill-detail] [role="switch"]')!
    expect(sw).not.toBeNull()
    expect(sw.getAttribute('aria-checked')).toBe('true')
    expect(sw.getAttribute('aria-label')).toBe(i18n.t('skills.enabled'))
    await act(async () => sw.click())
    expect(mute).toHaveBeenCalledWith('alpha', false)
    expect(sw.getAttribute('aria-checked')).toBe('false')
    expect(chip(row('alpha'))?.textContent).toBe(i18n.t('settingsUi.disabledChip'))
    await act(async () => sw.click())
    expect(mute).toHaveBeenLastCalledWith('alpha', true)
    expect(chip(row('alpha'))).toBeNull()
  })
})

describe('settings#mcp 列表行（#10）', () => {
  it('option 内无可交互后代；停用行 = 状态点 off + 「已停用」胶囊（副行不再重复写停用）', async () => {
    vi.spyOn(api, 'mcpEntries').mockResolvedValue([mcp('fs'), mcp('git', true)])
    vi.spyOn(api, 'mcpServices').mockResolvedValue([])
    await act(async () => root.render(<McpSection projectless={false} />))
    expect(options()).toHaveLength(2)
    for (const o of options()) expect(o.querySelectorAll(INTERACTIVE)).toHaveLength(0)
    const off = options().find((o) => o.textContent?.includes('git'))!
    expect(chip(off)?.textContent).toBe(i18n.t('settingsUi.disabledChip'))
    expect(off.querySelector('[data-slot="dot"]')?.getAttribute('data-tone')).toBe('off')
    expect(off.className).not.toMatch(/opacity/)
    expect(chip(options().find((o) => o.textContent?.includes('fs'))!)).toBeNull()
  })
})
