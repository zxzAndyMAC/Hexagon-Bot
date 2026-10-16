// gap D2（ui-redesign 16B，settings#team）：团队详情头部「休眠 / 唤醒」、保存行「移出团队」
// （AlertDialog danger）、副行「来自模板「X」· 本项目定制」。
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '../../i18n'
import { api, type AgentDetail, type RoleTemplate } from '../../api'
import { useUiStore } from '../../store'
import { TeamSection, templateOrigin } from './TeamSection'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const tpl = (name: string, duty: string): RoleTemplate => ({
  def: { name, duty, reviewer: null, model_slot: 'chat', globs: [], skills: [] }, origin: 'builtin',
})
const def = (duty: string): AgentDetail['def'] => ({ duty, reviewer: null, model_slot: 'chat', skills: [] })
const t = (k: string, o?: Record<string, unknown>) => i18n.t(k, o) as string

let el: HTMLDivElement
let root: Root
beforeEach(async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'team').mockResolvedValue([
    { id: 'a0', role: 'Frontend', status: 'sleeping', model_slot: 'chat', duty: 'ui' },
  ] as Awaited<ReturnType<typeof api.team>>)
  vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([tpl('Frontend', 'ui')])
  vi.spyOn(api, 'agentDetail').mockResolvedValue({
    agent_id: 'a0', role: 'Frontend', status: 'sleeping', model_slot: 'chat', custom: false,
    def: def('ui + pwa'), globs: [], grants: [],
  })
  vi.spyOn(api, 'removeAgent').mockResolvedValue(undefined)
  vi.spyOn(api, 'setAgentSleeping').mockResolvedValue(undefined)
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
})
afterEach(async () => {
  await act(async () => root.unmount()); el.remove(); vi.restoreAllMocks()
  useUiStore.setState({ confirmReq: null, toasts: [] })
})

const openAgent = async () => {
  await act(async () => root.render(<TeamSection />))
  await act(async () => el.querySelector<HTMLElement>('[role="option"]')!.click())
  await act(async () => {})
}

describe('templateOrigin（副行）', () => {
  it('同名模板且未改 → 来自模板；改过 → 追加本项目定制；无同名 → 本项目自建', () => {
    const tpls = [tpl('Frontend', 'ui')]
    expect(templateOrigin('Frontend', def('ui'), tpls, t)).toBe('From template “Frontend”')
    expect(templateOrigin('Frontend', def('ui2'), tpls, t)).toBe('From template “Frontend” · customized for this project')
    expect(templateOrigin('Ops', def('x'), tpls, t)).toBe('Created in this project')
    expect(templateOrigin('Frontend', null, tpls, t)).toBeNull()
  })
})

describe('团队详情动作（gap D2）', () => {
  it('头部副行 + 唤醒按钮；唤醒走 setAgentSleeping(false)', async () => {
    await openAgent()
    const pane = el.querySelector('[data-agent-detail]')!
    expect(pane.textContent).toContain('From template “Frontend” · customized for this project')
    const sleep = pane.querySelector<HTMLButtonElement>('[data-agent-sleep]')!
    expect(sleep.textContent).toContain(t('agent.wake'))
    await act(async () => sleep.click())
    expect(api.setAgentSleeping).toHaveBeenCalledWith('a0', false)
  })

  it('移出团队先出 danger 确认，确认后调 removeAgent 并关详情', async () => {
    await openAgent()
    await act(async () => el.querySelector<HTMLButtonElement>('[data-agent-remove]')!.click())
    const req = useUiStore.getState().confirmReq!
    expect(req.danger).toBe(true)
    expect(req.title).toBe('Remove “Frontend” from the team?')
    expect(api.removeAgent).not.toHaveBeenCalled()
    await act(async () => { await req.run() })
    expect(api.removeAgent).toHaveBeenCalledWith('a0')
    expect(useUiStore.getState().toasts.at(-1)?.text).toBe('Removed “Frontend”')
    expect(el.querySelector('[data-agent-detail]')).toBeNull()
  })

  it('core 拒绝（agent_busy）→ 原因码文案 toast，详情保留', async () => {
    vi.mocked(api.removeAgent).mockRejectedValue({ code: 'agent_busy', message: 'busy' })
    await openAgent()
    await act(async () => el.querySelector<HTMLButtonElement>('[data-agent-remove]')!.click())
    await act(async () => { await useUiStore.getState().confirmReq!.run() })
    const last = useUiStore.getState().toasts.at(-1)!
    expect(last.tone).toBe('err')
    expect(last.text).toContain(t('errors.agent_busy'))
    expect(el.querySelector('[data-agent-detail]')).not.toBeNull()
  })
})
