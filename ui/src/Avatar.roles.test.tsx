// owner-followups-1015 票 17：头像按角色存 + 实例覆盖 + 恢复默认（规范 components E4「头像」）。
// 断言负责人能看到 / 点到的结果：哪个入口改哪一层、「恢复默认」何时出现、界面头像跟着生效图变。
import { act, type ReactNode } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api, type AgentDetail, type ProvidersView, type RoleDef, type TeamRow } from './api'
import { useUiStore } from './store'
import { AgentTab } from './components/AgentTab'
import { RoleDetail } from './components/RoleDetail'
import { TeamSection } from './components/settings/TeamSection'
import { TooltipProvider } from './components/ui/tooltip'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const IMG = 'data:image/png;base64,iVBORw0KGgo='
const row = (over: Partial<TeamRow> = {}): TeamRow => ({
  id: 'a3', role: '前端', model_slot: 'code', status: 'active', avatar_hash: null, avatar_override: false,
  model: null, duty: null, superior: null, ...over,
})
const detail: AgentDetail = {
  agent_id: 'a3', role: '前端', status: 'active', model_slot: 'code', custom: false,
  def: { duty: 'ui', reviewer: null, model_slot: 'code', skills: [] }, globs: [], grants: [],
}

let el: HTMLDivElement
let root: Root
async function render(node: ReactNode) {
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
  await act(async () => root.render(<TooltipProvider>{node}</TooltipProvider>))
  await act(async () => {})
}

/** 选文件：FileReader 同步回 IMG（真 FileReader 在 jsdom 里是异步宏任务，测试不稳）。 */
async function pickFile(size = 10) {
  vi.stubGlobal('FileReader', class {
    onload: (() => Promise<void>) | null = null
    result = IMG
    readAsDataURL() { void Promise.resolve().then(() => this.onload?.()) }
  })
  const input = el.querySelector<HTMLInputElement>('input[type="file"]')!
  const file = new File(['x'], 'a.png', { type: 'image/png' })
  Object.defineProperty(file, 'size', { value: size })
  Object.defineProperty(input, 'files', { value: [file], configurable: true })
  await act(async () => { input.dispatchEvent(new Event('change', { bubbles: true })) })
  await act(async () => {})
}

const restoreBtn = () => el.querySelector<HTMLButtonElement>('[data-avatar-restore]')
const click = async (b: HTMLElement) => act(async () => { b.click() })

beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({ toasts: [] })
})
afterEach(async () => {
  if (root) await act(async () => root.unmount())
  el?.remove()
  vi.unstubAllGlobals(); vi.restoreAllMocks()
})

describe('两层读取在界面上的体现（store + dev mock）', () => {
  it('角色图让该角色实例换图；实例覆盖优先；实例恢复默认露出角色图；角色恢复默认回色块', async () => {
    useUiStore.setState({ avatarHashes: {}, avatars: {} })
    const inv = () => useUiStore.getState().invalidate('team')
    await inv()
    expect(useUiStore.getState().avatars.a3).toBeUndefined()

    await api.setRoleAvatar('前端', 'data:image/png;base64,ROLE')
    await inv()
    expect(useUiStore.getState().avatars.a3).toBe('data:image/png;base64,ROLE')
    expect(useUiStore.getState().avatars.a4).toBeUndefined() // 别的角色不受影响

    await api.setAgentAvatar('a3', 'data:image/png;base64,OWN')
    await inv()
    expect(useUiStore.getState().avatars.a3).toBe('data:image/png;base64,OWN')
    expect(useUiStore.getState().team.find((m) => m.id === 'a3')?.avatar_override).toBe(true)

    await api.clearAgentAvatar('a3')
    await inv()
    expect(useUiStore.getState().avatars.a3).toBe('data:image/png;base64,ROLE')

    await api.clearRoleAvatar('前端')
    await inv()
    expect(useUiStore.getState().avatars.a3).toBeUndefined()
  })
})

describe('画布 Agent 详情 = 实例覆盖', () => {
  beforeEach(() => {
    vi.spyOn(api, 'agentDetail').mockResolvedValue(detail)
    useUiStore.setState({ invalidate: async () => {} })
  })

  it('换图写实例层；无覆盖时不出「恢复默认」', async () => {
    useUiStore.setState({ team: [row()] })
    const set = vi.spyOn(api, 'setAgentAvatar').mockResolvedValue()
    const setRole = vi.spyOn(api, 'setRoleAvatar').mockResolvedValue()
    await render(<AgentTab agentId="a3" />)
    expect(el.querySelector('button[aria-label="Change avatar for this agent only"]')).toBeTruthy()
    expect(restoreBtn()).toBeNull()
    await pickFile()
    expect(set).toHaveBeenCalledWith('a3', IMG)
    expect(setRole).not.toHaveBeenCalled()
  })

  it('有覆盖时「恢复默认」删实例层', async () => {
    useUiStore.setState({ team: [row({ avatar_hash: 'h', avatar_override: true })] })
    const clear = vi.spyOn(api, 'clearAgentAvatar').mockResolvedValue()
    await render(<AgentTab agentId="a3" />)
    expect(restoreBtn()?.textContent).toContain('Restore default')
    await click(restoreBtn()!)
    expect(clear).toHaveBeenCalledWith('a3')
  })

  it('>2MB 本地拒绝：toast 报原因，不发请求', async () => {
    useUiStore.setState({ team: [row()] })
    const set = vi.spyOn(api, 'setAgentAvatar').mockResolvedValue()
    await render(<AgentTab agentId="a3" />)
    await pickFile(2 * 1024 * 1024 + 1)
    expect(set).not.toHaveBeenCalled()
    expect(useUiStore.getState().toasts.at(-1)?.text).toContain('over 2 MB')
  })

  it('服务端拒绝（解不开的图）走 errors.avatar_unreadable toast', async () => {
    useUiStore.setState({ team: [row()] })
    vi.spyOn(api, 'setAgentAvatar').mockRejectedValue({ code: 'avatar_unreadable', message: 'x' })
    await render(<AgentTab agentId="a3" />)
    await pickFile()
    expect(useUiStore.getState().toasts.at(-1)?.text).toContain('isn’t a readable PNG')
  })
})

describe('设置·团队 Agent 详情 = 角色头像', () => {
  beforeEach(() => {
    vi.spyOn(api, 'team').mockResolvedValue([row()])
    vi.spyOn(api, 'listRoleTemplates').mockResolvedValue([])
    vi.spyOn(api, 'agentDetail').mockResolvedValue(detail)
    vi.spyOn(api, 'listProviders').mockResolvedValue({ providers: [], slots: {} } as unknown as ProvidersView)
  })
  const openAgent = async () => {
    await render(<TeamSection />)
    await act(async () => el.querySelector<HTMLElement>('[role="option"]')!.click())
    await act(async () => {})
  }

  it('换图写角色层，之后出现「恢复默认」并删角色层', async () => {
    let stored: string | null = null
    vi.spyOn(api, 'roleAvatar').mockImplementation(async () => stored)
    const set = vi.spyOn(api, 'setRoleAvatar').mockImplementation(async (_r, u) => { stored = u })
    const setOwn = vi.spyOn(api, 'setAgentAvatar').mockResolvedValue()
    const clear = vi.spyOn(api, 'clearRoleAvatar').mockImplementation(async () => { stored = null })
    await openAgent()
    const pane = el.querySelector('[data-agent-detail]')!
    expect(pane.querySelector('button[aria-label="Change role avatar (all instances)"]')).toBeTruthy()
    expect(restoreBtn()).toBeNull()
    await pickFile()
    expect(set).toHaveBeenCalledWith('前端', IMG)
    expect(setOwn).not.toHaveBeenCalled()
    await click(restoreBtn()!)
    expect(clear).toHaveBeenCalledWith('前端')
    expect(restoreBtn()).toBeNull()
  })
})

describe('向导角色详情：头像留在向导内存', () => {
  const def: RoleDef = { name: '前端', duty: 'ui', reviewer: null, model_slot: 'default', globs: [], skills: [] }
  const doc = { providers: [], slots: {} } as unknown as ProvidersView
  const props = { def, tplName: '前端', names: ['前端'], doc, customized: false, aiDrafted: false, edited: false, busy: false, onChange: () => {}, onRestore: () => {} }

  it('换图回调 data URL；无图不出「恢复默认」', async () => {
    const onAvatar = vi.fn()
    await render(<RoleDetail {...props} avatarUrl={null} onAvatar={onAvatar} />)
    expect(el.querySelector('button[aria-label="Change role avatar (all instances)"]')).toBeTruthy()
    expect(restoreBtn()).toBeNull()
    await pickFile()
    expect(onAvatar).toHaveBeenCalledWith(IMG)
  })

  it('有图时「恢复默认」清回默认（null）', async () => {
    const onAvatar = vi.fn()
    await render(<RoleDetail {...props} avatarUrl={IMG} onAvatar={onAvatar} />)
    await click(restoreBtn()!)
    expect(onAvatar).toHaveBeenCalledWith(null)
  })
})
