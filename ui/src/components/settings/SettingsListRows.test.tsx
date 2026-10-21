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
import { ProviderManager } from '../ProviderManager'
import type { ProvidersView } from '../../gen/ProvidersView'

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

// owner-followups-1015 #30 第 5 项（2026-10-20 负责人裁决，票 10 后续）：服务商列表行同样按 E2「列表行不嵌开关」——
// 行内 Switch 撤掉、停用行用「已停用」胶囊（状态点已表 off，胶囊不再带点）、不再 opacity；启停开关移到详情头部。
const prov = (id: string, name: string, enabled: boolean) => ({
  id, name, kind: 'openai' as const, base_url: `https://${id}.example`, models: [], enabled, key_set: true,
})
const providers = (): ProvidersView => ({ providers: [prov('a', 'ReadyCo', true), prov('c', 'OffCo', false)], slots: {} })

describe('settings#models 服务商列表行（#30）', () => {
  beforeEach(() => {
    vi.spyOn(api, 'presetRoles').mockResolvedValue([])
    vi.spyOn(api, 'providerPresets').mockResolvedValue([])
  })

  it('option 内无可交互后代；停用行 = 状态点 off + 「已停用」胶囊，不整行半透明；启用行不出胶囊', async () => {
    vi.spyOn(api, 'listProviders').mockResolvedValue(providers())
    await act(async () => root.render(<ProviderManager />))
    const rows = options().filter((o) => /ReadyCo|OffCo/.test(o.textContent ?? ''))
    expect(rows).toHaveLength(2)
    for (const o of rows) expect(o.querySelectorAll(INTERACTIVE)).toHaveLength(0)
    const off = rows.find((o) => o.textContent?.includes('OffCo'))!
    expect(chip(off)?.textContent).toBe(i18n.t('settingsUi.disabledChip'))
    expect(off.querySelector('[data-slot="dot"]')?.getAttribute('data-tone')).toBe('off')
    expect(off.className).not.toMatch(/opacity/)
    expect(chip(rows.find((o) => o.textContent?.includes('ReadyCo'))!)).toBeNull()
  })

  it('详情头部开关切换启用：只存已保存的那份配置（不连带未保存的编辑），列表行随之出「已停用」', async () => {
    let doc = providers()
    vi.spyOn(api, 'listProviders').mockImplementation(async () => doc)
    const save = vi.spyOn(api, 'saveProvider').mockImplementation(async (p) => {
      doc = { ...doc, providers: doc.providers.map((x) => (x.id === p.id ? { ...x, enabled: p.enabled ?? x.enabled } : x)) }
    })
    await act(async () => root.render(<ProviderManager />))
    await act(async () => options().find((o) => o.textContent?.includes('ReadyCo'))!.click())
    // 未保存的改名不应被开关顺手存进去，也不应被开关的重新读档冲掉
    const name = el.querySelector<HTMLInputElement>(`input[aria-label="${i18n.t('providers.name')}"]`)!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(name, 'Renamed')
      name.dispatchEvent(new Event('input', { bubbles: true }))
    })
    const sw = el.querySelector<HTMLElement>('[data-provider-detail] [role="switch"]')!
    expect(sw).not.toBeNull()
    expect(sw.getAttribute('aria-checked')).toBe('true')
    expect(sw.getAttribute('aria-label')).toBe(i18n.t('providers.enabled'))
    await act(async () => sw.click())
    expect(save).toHaveBeenCalledTimes(1)
    expect(save.mock.calls[0][0]).toMatchObject({ id: 'a', name: 'ReadyCo', enabled: false })
    expect(el.querySelector<HTMLElement>('[data-provider-detail] [role="switch"]')!.getAttribute('aria-checked')).toBe('false')
    expect(name.value).toBe('Renamed')
    expect(chip(options().find((o) => o.textContent?.includes('ReadyCo'))!)?.textContent).toBe(i18n.t('settingsUi.disabledChip'))
  })
})
