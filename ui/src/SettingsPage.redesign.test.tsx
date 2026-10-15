// ui-redesign 票 11（components E1、E2、E5、E7；interaction §1）：设置外壳、列表宽度持久化、键盘分区、
// 自治开关、无项目空态的 App 级回归。
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api } from './api'
import { ACTIONS, bindingFor, resetBinding } from './keymap'
import { SETTINGS_LIST_KEY } from './settingsLayout'
import { useUiStore } from './store'
import { SettingsPage } from './components/SettingsPage'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let el: HTMLDivElement
let root: Root
beforeEach(async () => {
  await i18n.changeLanguage('en')
  localStorage.clear()
  vi.spyOn(api, 'logEnabled').mockResolvedValue(true)
  vi.spyOn(api, 'dataBoundary').mockResolvedValue({ credential_backend: 'keychain', project_open: true, recipients: [] })
  vi.spyOn(api, 'mcpEntries').mockResolvedValue([])
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  el = document.createElement('div'); document.body.appendChild(el); root = createRoot(el)
})
afterEach(async () => {
  await act(async () => root.unmount()); el.remove(); vi.restoreAllMocks()
  for (const a of ACTIONS) resetBinding(a.id)
})

const nav = async (id: string) => {
  await act(async () => el.querySelector<HTMLButtonElement>(`[data-nav="${id}"]`)!.click())
}

describe('设置外壳（E1）', () => {
  it('导航含「自治」（R-07），当前分区 aria-current，返回走 onBack', async () => {
    const back = vi.fn()
    await act(async () => root.render(<SettingsPage onBack={back} />))
    const ids = [...el.querySelectorAll('[data-nav]')].map((b) => b.getAttribute('data-nav'))
    expect(ids).toEqual(['general', 'keys', 'models', 'team', 'perms', 'autonomy', 'prompts', 'skills', 'mcp', 'usage', 'logs', 'about'])
    expect(el.querySelector('[data-nav="general"]')!.getAttribute('aria-current')).toBe('page')
    await act(async () => [...el.querySelectorAll('button')].find((b) => b.textContent?.includes('Back to workbench'))!.click())
    expect(back).toHaveBeenCalled()
  })

  it('无项目时项目态分区显示 A19 空态，动作「打开项目」回启动页', async () => {
    const back = vi.fn()
    await act(async () => root.render(<SettingsPage onBack={back} projectless />))
    await nav('autonomy')
    const empty = el.querySelector('[data-empty]')!
    expect(empty.textContent).toContain('Open a project first')
    await act(async () => [...empty.querySelectorAll('button')].find((b) => b.textContent === 'Open project')!.click())
    expect(back).toHaveBeenCalled()
  })
})

describe('列表 | 详情分割线（E2，interaction §1）', () => {
  it('键盘调宽后写入 hexagon.layout.settingsList，下次打开读回；Home 停在下限 220', async () => {
    await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="mcp" />))
    const sep = () => el.querySelector<HTMLElement>('[role="separator"]')!
    expect(sep().getAttribute('aria-valuenow')).toBe('280')
    await act(async () => sep().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, bubbles: true })))
    expect(JSON.parse(localStorage.getItem(SETTINGS_LIST_KEY)!)).toEqual({ width: 344 })
    expect(el.querySelector<HTMLElement>('[data-ld-list]')!.style.width).toBe('344px')
    await act(async () => root.unmount())
    root = createRoot(el)
    await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="skills" />))
    expect(sep().getAttribute('aria-valuenow')).toBe('344')
    await act(async () => sep().dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true })))
    expect(JSON.parse(localStorage.getItem(SETTINGS_LIST_KEY)!)).toEqual({ width: 220 })
  })

  it('列表是 listbox：↓ 选中下一项', async () => {
    vi.mocked(api.mcpEntries).mockResolvedValue([
      { name: 'one', command: 'a', args: [], env: {}, cwd: null, disabled: false, transport: 'stdio', url: null, headers: {}, origin: 'global' },
      { name: 'two', command: 'b', args: [], env: {}, cwd: null, disabled: false, transport: 'stdio', url: null, headers: {}, origin: 'global' },
    ])
    await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="mcp" />))
    const first = el.querySelector<HTMLElement>('[role="listbox"] [role="option"]')!
    await act(async () => first.click())
    expect(first.getAttribute('aria-selected')).toBe('true')
    await act(async () => first.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })))
    const options = [...el.querySelectorAll('[role="option"]')]
    expect(options[1].getAttribute('aria-selected')).toBe('true')
    expect(el.querySelector<HTMLInputElement>('input[aria-label="Command"]')!.value).toBe('b')
  })
})

describe('键盘分区（E7）', () => {
  it('从 keymap 自动列出全部动作（含 02、03 新增和改名的动作；splitEditor 已撤，R-04）', async () => {
    await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="keys" />))
    const rows = [...el.querySelectorAll('[data-key-row]')].map((r) => r.getAttribute('data-key-row'))
    expect(rows.sort()).toEqual(ACTIONS.map((a) => a.id).sort())
    for (const id of ['toggleSidebar', 'toggleCanvas', 'canvasFullWidth', 'focusMode']) expect(rows).toContain(id)
    expect(rows).not.toContain('splitEditor')
    expect(rows).not.toContain('toggleRail')
  })

  it('录制：冲突就地红字不写入；Esc 取消；新组合写入', async () => {
    await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="keys" />))
    const row = () => el.querySelector<HTMLElement>('[data-key-row="settings"]')!
    const record = () => row().querySelector<HTMLButtonElement>('button[aria-label^="Record"]')!
    const before = bindingFor('settings')
    // 冲突：mod+K 已被命令面板占用
    await act(async () => record().click())
    expect(row().querySelector('[data-recording]')).not.toBeNull()
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, metaKey: true, bubbles: true })))
    expect(row().querySelector('[role="alert"]')!.textContent).toContain('Already used by')
    expect(bindingFor('settings')).toBe(before)
    // Esc 取消
    await act(async () => record().click())
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })))
    expect(row().querySelector('[data-recording]')).toBeNull()
    // 系统保留组合不可录
    await act(async () => record().click())
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'q', ctrlKey: true, metaKey: true, bubbles: true })))
    expect(row().querySelector('[role="alert"]')!.textContent).toContain('reserved')
    // 写入
    await act(async () => record().click())
    await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'F8', bubbles: true })))
    expect(bindingFor('settings')).toBe('F8')
  })

  it('搜索动作过滤行', async () => {
    await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="keys" />))
    const search = el.querySelector<HTMLInputElement>('input[aria-label="Search actions"]')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(search, 'toggleSidebar')
      search.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect([...el.querySelectorAll('[data-key-row]')].map((r) => r.getAttribute('data-key-row'))).toEqual(['toggleSidebar'])
  })
})

describe('自治（R-07：E1 行 + Switch）', () => {
  it('切到 live 先走确认，不乐观翻转；确认后才写入', async () => {
    vi.spyOn(api, 'reviewerMode').mockResolvedValue('shadow')
    const set = vi.spyOn(api, 'setReviewerMode').mockResolvedValue(undefined)
    await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="autonomy" />))
    const sw = el.querySelector<HTMLButtonElement>('[role="switch"]')!
    expect(sw.getAttribute('aria-checked')).toBe('false')
    await act(async () => sw.click())
    expect(set).not.toHaveBeenCalled()
    expect(sw.getAttribute('aria-checked')).toBe('false')
    const req = useUiStore.getState().confirmReq!
    expect(req.title).toBe(i18n.t('auto.liveTitle'))
    await act(async () => { await req.run() })
    expect(set).toHaveBeenCalledWith('live')
    expect(sw.getAttribute('aria-checked')).toBe('true')
    useUiStore.getState().clearConfirm()
  })
})

describe('权限（E5）', () => {
  it('托管只显示状态与报告，不放开关；规则表可按工具筛选', async () => {
    vi.spyOn(api, 'projectDelegation').mockResolvedValue({ project_root: '/p', enabled: false, paused: false } as never)
    vi.spyOn(api, 'managedHistory').mockResolvedValue([])
    vi.spyOn(api, 'desktopStatus').mockRejectedValue(new Error('n/a'))
    vi.spyOn(api, 'permissionRules').mockResolvedValue([
      { id: 'r1', project_shared: false, agent_id: null, tool: 'bash', shape: 'npm test', domain: null, effect: 'allow', scope: 'project', created_at: '2026-10-01', network_allowed: false, background_allowed: false, session_name: null },
      { id: 'r2', project_shared: false, agent_id: null, tool: 'web_fetch', shape: 'developer.mozilla.org', domain: null, effect: 'allow', scope: 'project', created_at: '2026-10-02', network_allowed: true, background_allowed: false, session_name: null },
    ])
    await act(async () => root.render(<SettingsPage onBack={() => {}} initialSection="perms" />))
    const block = el.querySelector('[data-managed-block]')!
    expect(block.querySelector('[role="switch"]')).toBeNull()
    expect(block.querySelector('[data-delegation-state="off"]')).not.toBeNull()
    expect(el.textContent).toContain('Remembered rules (2)')
    const filter = el.querySelector<HTMLInputElement>('input[aria-label="Filter by role or tool"]')!
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(filter, 'web_fetch')
      filter.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(el.querySelector('[data-rule="r1"]')).toBeNull()
    expect(el.querySelector('[data-rule="r2"]')).not.toBeNull()
  })
})
