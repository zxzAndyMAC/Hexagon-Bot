import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { ProjectApprovalMode } from './components/ProjectApprovalMode'
import { ManagedPill } from './components/ManagedPill'
import { ProjectDelegation } from './components/ProjectDelegation'
import { Header } from './components/shell/Header'
import { Sidebar } from './components/shell/Sidebar'
import { TooltipProvider } from './components/ui/tooltip'
import { api } from './api'
import { useUiStore } from './store'
import i18n from './i18n'
import type { DelegationStatus } from './gen/DelegationStatus'
;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
const off: DelegationStatus = { enabled: false, paused: false, generation: 0, delegation_id: null }
const on: DelegationStatus = { enabled: true, paused: false, generation: 1, delegation_id: 'g1' }

beforeEach(async () => {
  await i18n.changeLanguage('en')
  vi.spyOn(api, 'approvalMode').mockResolvedValue({ project_id: 'p1', project_root: '/proj', mode: 'assisted' })
  useUiStore.setState({ projectRoot: '/proj' })
  vi.spyOn(api, 'managedHistory').mockResolvedValue([])
  vi.spyOn(api, 'managedExternalOps').mockResolvedValue([])
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([])
})
afterEach(async () => { await act(async () => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })

async function mount(node: React.ReactNode) {
  document.body.appendChild(el); root = createRoot(el)
  await act(async () => root.render(node))
}

it('delegation is a separate switch in the approval menu, explained once, never optimistic', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(off)
  let resolve!: (value: DelegationStatus) => void
  const save = vi.spyOn(api, 'setProjectDelegation').mockImplementation(() => new Promise(r => { resolve = r }))
  await mount(<ProjectApprovalMode />)
  const trigger = el.querySelector<HTMLButtonElement>('.approval-mode-trigger')!
  expect(trigger.textContent).toBe('Help me approve')
  await act(async () => trigger.click())
  expect(el.querySelectorAll('[role="menuitemradio"]')).toHaveLength(3)
  const toggle = el.querySelector<HTMLInputElement>('input[role="switch"]')!
  expect(toggle.checked).toBe(false)
  await act(async () => toggle.click())
  expect(save).not.toHaveBeenCalled()
  // 票 08（components D10 / A17）：说明从原生 <dialog> 换到共享 AlertDialog，经 portal 挂到 body。
  const dialog = document.querySelector('.managed-confirm')!
  expect(dialog.textContent).toContain('The system will not')
  // 票 08（D10）：上限从一句 limits 拆成三行信息（refresh / usage / flag），句首大写。
  expect(dialog.textContent).toMatch(/at most 3 correction rounds/i)
  // ADR0087 (ticket 01): full release is stated honestly before turning on.
  expect(dialog.textContent).toContain('only go to the system Trash')
  expect(dialog.textContent).toContain('MCP servers and skills installed in this project')
  // ADR0087 实施修订 2: delegation never lifts a skill's manual-only gate.
  expect(dialog.textContent).toContain('skills marked manual-only')
  expect(dialog.querySelector('[data-external-deletion]')!.textContent).toContain('cannot block deletions')
  expect(dialog.querySelector('[data-linux-limit]')!.textContent).toContain('Linux')
  // 票 08（A17）：「取消」按钮带 Esc 键位块。
  expect(document.activeElement?.textContent).toBe('CancelEsc')
  const confirm = [...dialog.querySelectorAll('button')].find(b => b.textContent === 'Turn on delegation')!
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  await act(async () => confirm.click())
  expect(save).toHaveBeenCalledWith(true, '/proj')
  expect(trigger.textContent).toBe('Help me approve', /* host has not answered yet */)
  await act(async () => resolve(on))
  expect(trigger.textContent).toBe('Help me approve | Delegation')
})

it('the resident pill wears the accent identity, pauses and turns off through the host', async () => {
  // 票 08（components D7，R-08 拍板）：胶囊从中性灰改为 accent 身份色（仍不是待决的橙）；
  // 弹层改 A16 Popover（portal → 查 document）；关闭后 toast 转回人工件数，报告作收场弹出。
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([{ id: 'q1', kind: 'permission', agent_id: 'a0', payload: {}, state: 'queued' }])
  const pause = vi.spyOn(api, 'setProjectDelegationPaused').mockResolvedValue({ ...on, paused: true, generation: 2 })
  const disable = vi.spyOn(api, 'setProjectDelegation').mockResolvedValue(off)
  useUiStore.setState({ toasts: [] })
  await mount(<ManagedPill />)
  const pill = el.querySelector<HTMLButtonElement>('.managed-pill')!
  expect(pill.textContent).toContain('Delegating')
  expect(pill.className).toContain('mg-chip')
  expect(pill.className).not.toContain('amber')
  await act(async () => pill.click())
  const menu = document.querySelector('[data-managed-menu]')!
  expect(menu.textContent).toContain('1 item(s)')
  const pauseButton = [...menu.querySelectorAll('button')].find(b => b.textContent?.startsWith('Pause delegation'))!
  vi.spyOn(api, 'projectDelegation').mockResolvedValue({ ...on, paused: true, generation: 2 })
  await act(async () => pauseButton.click())
  expect(pause).toHaveBeenCalledWith(true, '/proj')
  expect(el.querySelector('[data-managed-state]')!.getAttribute('data-managed-state')).toBe('paused')
  expect(el.querySelector('.managed-pill')!.className).toContain('paused')
  const offButton = [...document.querySelectorAll('[data-managed-menu] button')].find(b => b.textContent?.startsWith('Turn off project delegation'))!
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(off)
  await act(async () => (offButton as HTMLButtonElement).click())
  expect(disable).toHaveBeenCalledWith(false, '/proj')
  expect(el.querySelector('.managed-pill')).toBeNull()
  expect(useUiStore.getState().toasts.map(x => x.text)).toContain('Delegation off · 1 item(s) back with you')
  expect(document.querySelector('[data-managed-report]')).not.toBeNull()
})

it('pause and turn-off keys fire in any modal scope; there is no direct turn-on key', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  const pause = vi.spyOn(api, 'setProjectDelegationPaused').mockResolvedValue({ ...on, paused: true, generation: 2 })
  const { setBinding, resetBinding, bindingFor, isMac } = await import('./keymap')
  setBinding('managedPause', 'alt+mod+p')
  try {
    useUiStore.setState({ modalScope: 'settings' })
    await mount(<ManagedPill />)
    await act(async () => { window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', code: 'KeyP', altKey: true, metaKey: isMac, ctrlKey: !isMac, bubbles: true })) })
    expect(pause).toHaveBeenCalledWith(true, '/proj')
  } finally { resetBinding('managedPause'); useUiStore.setState({ modalScope: 'workbench' }) }
  resetBinding('managedEnable'); resetBinding('confirmManaged')
  expect(bindingFor('managedEnable')).toBe('')
  expect(bindingFor('confirmManaged')).toBe('')
})

it('the pending badge counts only owner items; delegated ones show in the pill', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedProcessing').mockResolvedValue([{ id: 'q9', kind: 'permission', agent_id: 'a0', payload: {}, state: 'queued' }])
  vi.spyOn(api, 'sandboxStatus').mockResolvedValue({ available: false, mode: 'none', note: '' })
  // The host's pending read already excludes delegated cards (owner_pending).
  useUiStore.setState({ projectName: 'p', packName: 'x', mode: 'pack', pending: [], reviewRows: [], team: [], projectRoot: '/proj' })
  // ADR 0088（票 02）：顶栏撤掉——待决徽标迁到侧栏「待决」，托管胶囊原样迁到头部。口径不变。
  await mount(<TooltipProvider><Sidebar onSearch={() => {}} onSettings={() => {}} onProjectClosed={() => {}} /><Header /></TooltipProvider>)
  expect(el.querySelector('[data-header] .managed-pill')).not.toBeNull()
  expect(el.querySelector('[data-pending-count]')).toBeNull()
  expect(el.querySelector('.managed-pill')).not.toBeNull()
})

it('settings shows status and the report, not a second switch', async () => {
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedHistory').mockResolvedValue([
    { event_id: 9, card_id: 'q7', card_kind: 'permission', action: 'deny', reason: 'budget_exhausted', via: 'host', outcome: 'denied', actor: 'managed:g:3', result: 'done', scope: 'in_scope', evidence: 'model_choice' },
  ])
  await mount(<ProjectDelegation />)
  expect(el.querySelector('input[role="switch"]')).toBeNull()
  expect(el.textContent).toContain('approval mode menu')
  expect(el.textContent).toContain('Refused')
  // 票 08（D9）：一句「Recent: 1 done …」换成 KPI 四格。
  expect(el.querySelector('[data-managed-kpi]')!.textContent).toContain('Done1')
})

it('the report lists external operations with honest verification and jumps to the decision row', async () => {
  // Managed-followups ticket 05 (ADR0087): app/MCP/browser are unverified; a
  // host trash is recoverable and says where the item is.
  vi.spyOn(api, 'projectDelegation').mockResolvedValue(on)
  vi.spyOn(api, 'managedHistory').mockResolvedValue([
    { event_id: 11, card_id: 'q1', card_kind: 'permission', action: 'allow', reason: 'model_choice', via: 'model', outcome: 'allowed_and_continued', actor: 'managed:g:1', result: 'done', scope: 'in_scope', evidence: 'model_choice' },
  ])
  vi.spyOn(api, 'managedExternalOps').mockResolvedValue([
    { event_id: 11, card_id: 'q1', kind: 'local_trash', target: '/Users/me/old.png', action: 'move_to_trash', decision: 'allow', result: 'done', at: '2026-10-11 12:00:00', verification: 'host_verified_recoverable', trash_path: '/Users/me/.Trash/old.png' },
    { event_id: 12, card_id: 'q2', kind: 'mcp', target: 'figma:update_node', action: 'update_node', decision: 'allow', result: 'done', at: '2026-10-11 12:01:00', verification: 'host_unverified_deletion', trash_path: null },
  ])
  const jumped: number[] = []
  const listen = (event: Event) => jumped.push((event as CustomEvent<number>).detail)
  window.addEventListener('hexagon:timeline-jump', listen)
  try {
    await mount(<ProjectDelegation />)
    const section = el.querySelector('[data-external-ops]')!
    expect(section.textContent).toContain('External operations')
    expect(el.querySelector('[data-external-kind="local_trash"]')!.textContent).toContain('Recoverable from Trash')
    expect(el.querySelector('[data-external-kind="local_trash"]')!.textContent).toContain('/Users/me/.Trash/old.png')
    expect(el.querySelector('[data-external-kind="mcp"]')!.textContent).toContain('Host did not verify')
    // 票 08（D9 核实标记）：chip 用短标签，完整说明在 title。
    expect(el.querySelector('[data-external-kind="mcp"] [title]')!.getAttribute('title')).toBe('Host did not verify whether anything was deleted')
    const button = [...el.querySelectorAll<HTMLButtonElement>('[data-external-kind="mcp"] button')].at(-1)!
    expect(button.title).toBe('Show in timeline')
    await act(async () => button.click())
    expect(jumped).toEqual([12])
  } finally { window.removeEventListener('hexagon:timeline-jump', listen) }
})
