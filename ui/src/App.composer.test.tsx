import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import App from './App'
import { api } from './api'
import { useUiStore, type WorkTab } from './store'
import i18n from './i18n'

// happy-dom has no canvas renderer; exercise real tabs with the chart SDK stubbed.
vi.mock('echarts/core', () => ({ use: vi.fn(), init: () => ({ setOption: vi.fn(), resize: vi.fn(), dispose: vi.fn() }) }))

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let root: ReturnType<typeof createRoot>
let el: HTMLDivElement
const initial = useUiStore.getState()
const timeline: WorkTab = { id: 'timeline', kind: 'timeline', title: '' }

function visible(element: HTMLElement) {
  for (let node: HTMLElement | null = element; node; node = node.parentElement) {
    if (node.hidden || node.style.display === 'none') return false
  }
  return true
}

beforeEach(async () => {
  await i18n.changeLanguage('en')
  useUiStore.setState({ tabs: [timeline], activeTab: 'timeline', splitOpen: false, team: [], pending: [], timeline: [], modalScope: 'workbench', intakeDraft: false })
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
  await act(async () => root.unmount())
  el.remove()
  vi.restoreAllMocks()
  useUiStore.setState(initial)
})

// Owner 2026-10-06: Composer formerly occupied every tab's content area.
// Switching tabs must release that space without deleting the unsent draft.
it('shows the input only on the timeline and keeps the draft when returning', async () => {
  const input = el.querySelector<HTMLTextAreaElement>('#composer-input')!
  expect(visible(input)).toBe(true)
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, 'Unsent draft')
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  const otherTabs: WorkTab[] = [
    { id: 'file:README.md', kind: 'file', title: 'README.md', path: 'README.md' },
    { id: 'art:spec.md', kind: 'artifact', title: 'spec.md', path: 'spec.md' },
    { id: 'agent:a1', kind: 'agent', title: 'Agent', agentId: 'a1' },
    { id: 'usage', kind: 'usage', title: 'Usage' },
    { id: 'diff:spec', kind: 'diff', title: 'Diff', patchText: '' },
  ]
  for (const tab of otherTabs) {
    await act(async () => useUiStore.getState().openTab(tab))
    expect(visible(input), tab.kind).toBe(false)
  }
  await act(async () => useUiStore.getState().setActiveTab('timeline'))
  const restored = el.querySelector<HTMLTextAreaElement>('#composer-input')!
  expect(visible(restored)).toBe(true)
  expect(restored.value).toBe('Unsent draft')
})

it('returns to the timeline when the focus-input shortcut is used from another tab', async () => {
  await act(async () => useUiStore.getState().openTab({ id: 'diff:spec', kind: 'diff', title: 'Diff', patchText: '' }))
  await act(async () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, bubbles: true, cancelable: true })))
  await act(async () => { await new Promise<void>(resolve => requestAnimationFrame(() => resolve())) })
  expect(useUiStore.getState().activeTab).toBe('timeline')
  expect(document.activeElement).toBe(el.querySelector('#composer-input'))
})
