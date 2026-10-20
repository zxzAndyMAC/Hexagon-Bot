// 成员行 + 消息右键菜单（components A16b 第一批，owner-followups-1015 #08）。
// 成员行：打开详情 · @ 点名 · 复制名字；消息：复制 · 引用到输入条 · 在节点轨中定位。
// 有选区落在该消息内时复制 / 引用只作用于选区；消息不在节点轨上时「定位」禁用写原因；
// 待决卡与裁决按钮上右键不出任何菜单；新动作默认空绑定、进命令面板。
import { act, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import App from './App'
import { api, type EventKind, type TimelineItem } from './api'
import { useUiStore } from './store'
import { formatBinding, isMac, resetBinding, setBinding } from './keymap'
import type { TeamRow } from './gen/TeamRow'
import i18n from './i18n'

vi.mock('./components/PendingDock', () => ({ PendingDock: () => null }))
vi.mock('react-virtuoso', () => ({
  Virtuoso: ({ data, itemContent, scrollerRef }: { data: unknown[]; itemContent: (i: number, row: unknown) => ReactNode; scrollerRef: (node: HTMLElement | null) => void }) =>
    <div data-scroller ref={scrollerRef}>{data.map((row, index) => <div key={index}>{itemContent(index, row)}</div>)}</div>,
}))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const member = (id: string, role: string): TeamRow => ({
  id, role, model_slot: null, status: 'active', avatar_hash: null, avatar_override: false, model: null, duty: null, superior: null,
})
const coder = member('a1', 'Coder')
const qa1 = member('q1', 'QA')
const qa2 = member('q2', 'QA')

const item = (id: number, kind: EventKind, author: string, body: string): TimelineItem => ({
  event: { id, project_id: 'p', kind, agent_id: author !== 'owner' ? author : null, stage_run_id: null, payload: { body }, created_at: '2026-10-14T14:02:00Z' },
  message: { id, author, body, tokens: [], attachments: [], element_refs: [], model: null, created_at: '', thinking: '' },
})
const ownerMsg = item(11, 'owner_message', 'owner', 'Please build the **todo** list')
const agentMsg = item(12, 'agent_message', 'a1', 'Done: the list renders\nsecond line')

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()
const writeText = vi.fn()

const memberRow = (id: string) => el.querySelector(`[data-member="${id}"]`) as HTMLElement
const messageEl = (eventId: number) => el.querySelector(`[data-msg-event="${eventId}"]`) as HTMLElement
const menu = () => document.querySelector('[data-slot="context-menu-content"]') as HTMLElement | null
const rows = () => [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')]
const row = (id: string) => rows().find((r) => r.dataset.ctxItem === id)!
const composer = () => document.getElementById('composer-input') as HTMLTextAreaElement
const rightClick = (target: Element) => act(async () => {
  const r = target.getBoundingClientRect()
  target.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 2, clientX: r.left + 4, clientY: r.top + 4 }))
  target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: r.left + 4, clientY: r.top + 4 }))
})
const keyOn = (target: Element, init: KeyboardEventInit) => act(async () => {
  target.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }))
})
const pick = (id: string) => act(async () => { row(id).click() })
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); await new Promise((r) => requestAnimationFrame(() => r(null))) })
/** 在 node 内选中 [start, end) 的文字。 */
const selectIn = (node: Node, start: number, end: number) => {
  const range = document.createRange()
  range.setStart(node, start)
  range.setEnd(node, end)
  const sel = window.getSelection()!
  sel.removeAllRanges()
  sel.addRange(range)
}
const textNode = (host: Element, needle: string): Text => {
  const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT)
  for (let n = walker.nextNode(); n; n = walker.nextNode()) if (n.textContent!.includes(needle)) return n as Text
  throw new Error(`no text node with ${needle}`)
}

beforeEach(async () => {
  await i18n.changeLanguage('en')
  writeText.mockReset().mockResolvedValue(undefined)
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
  useUiStore.setState({
    projectRoot: null, projectGeneration: null, projectEpoch: 0, tabs: [], activeTab: null, canvasOpen: false, usageView: false, focusSnapshot: null,
    canvasLayout: { width: null, full: false }, team: [coder, qa1, qa2], pending: [], artifacts: [], timeline: [ownerMsg, agentMsg],
    modalScope: 'workbench', intakeDraft: false, usageTotal: null, confirmReq: null, sessionSummary: null, sessionError: null, fileEdits: {},
    streams: {}, timelineFacts: null,
  })
  vi.spyOn(api, 'projectOpen').mockResolvedValue(true)
  vi.spyOn(api, 'runOpeningIntake').mockResolvedValue()
  vi.spyOn(api, 'mcpServices').mockResolvedValue([])
  vi.spyOn(useUiStore.getState(), 'refresh').mockResolvedValue()
  vi.spyOn(useUiStore.getState(), 'refreshFast').mockResolvedValue()
  el = document.createElement('div')
  document.body.appendChild(el)
  root = createRoot(el)
  await act(async () => root.render(<App />))
})

afterEach(async () => {
  window.getSelection()?.removeAllRanges()
  await act(async () => root.unmount())
  el.remove()
  vi.restoreAllMocks()
  resetBinding('mentionMember')
  resetBinding('copyMessage')
  useUiStore.setState(initial)
})

describe('成员行右键菜单（A16b 第一批）', () => {
  it('打开详情 · @ 点名 · 复制名字，依此顺序', async () => {
    await rightClick(memberRow('a1'))
    expect(menu()).not.toBeNull()
    expect(rows().map((r) => r.dataset.ctxItem)).toEqual(['openDetail', 'mention', 'copyName'])
    expect(rows().map((r) => r.textContent)).toEqual(['Open details', '@ Mention', 'Copy name'])
  })

  it('右键不改选中：不打开该成员，只给行出焦点环标记', async () => {
    await rightClick(memberRow('a1'))
    expect(useUiStore.getState().tabs).toEqual([])
    expect(memberRow('a1').dataset.state).toBe('open')
  })

  it('打开详情 = 在画布打开该 Agent', async () => {
    await rightClick(memberRow('a1'))
    await pick('openDetail')
    expect(useUiStore.getState().activeTab).toBe('agent:a1')
  })

  it('@ 点名：把 @角色 插进输入条并聚焦；同名角色写成 角色[id]', async () => {
    await rightClick(memberRow('a1'))
    await pick('mention')
    await settle()
    expect(composer().value).toBe('@Coder ')
    expect(document.activeElement).toBe(composer())
    await rightClick(memberRow('q2'))
    await pick('mention')
    await settle()
    expect(composer().value).toBe('@Coder @QA[q2] ')
  })

  it('复制名字：写入角色名并出回执', async () => {
    await rightClick(memberRow('a1'))
    await pick('copyName')
    expect(writeText).toHaveBeenCalledWith('Coder')
    expect(useUiStore.getState().toasts.at(-1)?.text).toBe('Name copied')
  })

  it('Shift+F10 在焦点成员行上打开菜单；Esc 后焦点回该行', async () => {
    act(() => memberRow('a1').focus())
    await keyOn(memberRow('a1'), { key: 'F10', shiftKey: true })
    expect(menu()).not.toBeNull()
    await keyOn(menu()!, { key: 'Escape' })
    await settle()
    expect(document.activeElement).toBe(memberRow('a1'))
  })

  it('绑定后行右侧显示键位；键位作用于焦点成员行', async () => {
    setBinding('mentionMember', 'alt+mod+shift+m')
    await rightClick(memberRow('a1'))
    expect(row('mention').querySelector('[data-ctx-hint]')!.textContent).toBe(formatBinding('alt+mod+shift+m'))
    await keyOn(menu()!, { key: 'Escape' })
    await settle()
    act(() => memberRow('a1').focus())
    await keyOn(memberRow('a1'), { key: 'm', altKey: true, shiftKey: true, ...(isMac ? { metaKey: true } : { ctrlKey: true }) })
    await settle()
    expect(composer().value).toBe('@Coder ')
  })
})

describe('消息右键菜单（A16b 第一批）', () => {
  it('负责人消息：复制 · 引用到输入条 · 在节点轨中定位；在节点轨上 → 定位可用', async () => {
    await rightClick(messageEl(11))
    expect(rows().map((r) => r.dataset.ctxItem)).toEqual(['copy', 'quote', 'locate'])
    expect(row('locate').getAttribute('aria-disabled')).toBeNull()
  })

  it('Agent 消息不在节点轨上：定位禁用并写原因', async () => {
    await rightClick(messageEl(12))
    expect(rows().map((r) => r.dataset.ctxItem)).toEqual(['copy', 'quote', 'locate'])
    expect(row('locate').getAttribute('aria-disabled')).toBe('true')
    expect(row('locate').textContent).toBe('Locate in node railNot on the node rail')
  })

  it('无选区：复制整条原文', async () => {
    await rightClick(messageEl(12))
    await pick('copy')
    expect(writeText).toHaveBeenCalledWith('Done: the list renders\nsecond line')
    expect(useUiStore.getState().toasts.at(-1)?.text).toBe('Message copied')
  })

  it('有选区落在该消息内：复制只作用于选区', async () => {
    selectIn(textNode(messageEl(12), 'the list'), 6, 14)
    await rightClick(messageEl(12))
    await pick('copy')
    expect(writeText).toHaveBeenCalledWith('the list')
    expect(useUiStore.getState().toasts.at(-1)?.text).toBe('Selection copied')
  })

  it('选区在别的消息里：仍作用于整条', async () => {
    selectIn(textNode(messageEl(11), 'Please'), 0, 6)
    await rightClick(messageEl(12))
    await pick('copy')
    expect(writeText).toHaveBeenCalledWith('Done: the list renders\nsecond line')
  })

  it('引用到输入条：整条按引用块填入输入条，空一行等负责人接着写', async () => {
    await rightClick(messageEl(12))
    await pick('quote')
    await settle()
    expect(composer().value).toBe('> Done: the list renders\n> second line\n\n')
    expect(document.activeElement).toBe(composer())
  })

  it('引用选区：只引用选中的那段', async () => {
    selectIn(textNode(messageEl(12), 'the list'), 6, 14)
    await rightClick(messageEl(12))
    await pick('quote')
    await settle()
    expect(composer().value).toBe('> the list\n\n')
  })

  it('在节点轨中定位：聚焦这条消息的刻度', async () => {
    await rightClick(messageEl(11))
    await pick('locate')
    await settle()
    const tick = document.activeElement as HTMLElement
    expect(tick.dataset.tick).toBeDefined()
    expect(tick.dataset.tickEvent).toBe('11')
  })

  it('消息菜单里没有任何裁决项', async () => {
    await rightClick(messageEl(11))
    const labels = rows().map((r) => r.textContent ?? '')
    for (const banned of ['Approve', 'Reject', 'Allow', 'Deny', 'Run once']) expect(labels.some((l) => l.includes(banned))).toBe(false)
  })

  it('键位作用于焦点所在的消息（复制钮在消息里）', async () => {
    setBinding('copyMessage', 'alt+mod+shift+c')
    const btn = messageEl(12).querySelector('button.tl-copy') as HTMLElement
    act(() => btn.focus())
    await keyOn(btn, { key: 'c', altKey: true, shiftKey: true, ...(isMac ? { metaKey: true } : { ctrlKey: true }) })
    expect(writeText).toHaveBeenCalledWith('Done: the list renders\nsecond line')
  })
})

describe('新动作进命令面板（A16b「定位」）', () => {
  const palette = async () => {
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', bubbles: true, cancelable: true, ...(isMac ? { metaKey: true } : { ctrlKey: true }) }))
    })
    return [...document.querySelectorAll('[data-command-palette] [cmdk-item]')].map((n) => n.textContent ?? '')
  }

  it('焦点在成员行时面板有 @ 点名 / 复制名字', async () => {
    act(() => memberRow('a1').focus())
    const items = await palette()
    expect(items.some((x) => x.startsWith('@ Mention Coder'))).toBe(true)
    expect(items.some((x) => x.startsWith('Copy name: Coder'))).toBe(true)
  })

  it('焦点在消息里时面板有复制消息 / 引用 / 定位；焦点在别处则不列', async () => {
    const items = await palette()
    expect(items.some((x) => x.startsWith('Copy message'))).toBe(false)
    await keyOn(document.activeElement!, { key: 'Escape' })
    await settle()
    expect(document.querySelector('[data-command-palette]')).toBeNull()
    const btn = messageEl(11).querySelector('button.tl-copy') as HTMLElement
    act(() => btn.focus())
    const again = await palette()
    expect(again.some((x) => x.startsWith('Copy message'))).toBe(true)
    expect(again.some((x) => x.startsWith('Quote in composer'))).toBe(true)
    expect(again.some((x) => x.startsWith('Locate in node rail'))).toBe(true)
  })
})
