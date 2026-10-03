import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { PendingCard } from './components/PendingCards'
import { api, type PendingQuestion } from './api'
import { useUiStore } from './store'
import i18n from './i18n'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(async () => { await act(async () => root?.unmount()); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })
const card: PendingQuestion = { id: 'q-prefill', kind: 'permission', agent_id: 'a0', state: 'queued',
  payload: { tool: 'web_fetch', input: { url: 'https://example.com/docs/a' }, safety_net: false } }

// ADR 0079: replace editable shapes with an explicit host-generated project choice.
it('shows a readable scope and only grants project permission when selected', async () => {
  await i18n.changeLanguage('zh-CN')
  vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValue({ shape: 'https://example.com/docs/*', generalized: true, tool: 'web_fetch', project_id: 'p', agent_id: 'a0' })
  const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue(undefined)
  const project = vi.spyOn(api, 'allowProjectPermission').mockResolvedValue(undefined)
  vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue(undefined)
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={card} top />) })
  expect(el.querySelector('input')).toBeNull()
  expect(answer).not.toHaveBeenCalled()
  expect(project).not.toHaveBeenCalled()
  await act(async () => { el.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')!.click() })
  expect(el.textContent).toContain('本项目所有角色')
  expect(el.textContent).toContain('https://example.com/docs/')
  const choose = el.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')[1]!
  await act(async () => choose.click())
  expect(project).toHaveBeenCalledExactlyOnceWith('q-prefill')
  expect(answer).not.toHaveBeenCalled()
})

it('new cards start closed and unsupported actions only offer once and deny', async () => {
  vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValueOnce({ shape: 'exact:npm test', generalized: false, tool: 'bash', project_id: 'p', agent_id: 'a0' }).mockResolvedValueOnce(null)
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={card} top />) })
  await act(async () => { el.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')!.click() })
  await act(async () => { root.render(<PendingCard q={{ ...card, id: 'q-next' }} top />) })
  expect(el.querySelector('[role="menu"]')).toBeNull()
  expect(el.querySelector('[aria-haspopup="menu"]')).toBeNull()
  expect(el.textContent).toContain('允许一次')
  expect(el.textContent).toContain('拒绝')
})

// ADR 0079: execution failure does not roll back the durable grant or peer answers.
it('refreshes consumed cards when project execution reports an error', async () => {
  vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValue({ shape: 'exact:npm test', generalized: false, tool: 'bash', project_id: 'p', agent_id: 'a0' })
  vi.spyOn(api, 'allowProjectPermission').mockRejectedValue(new Error('execution failed'))
  const refresh = vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue(undefined)
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={card} top />) })
  await act(async () => { el.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')!.click() })
  await act(async () => { el.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')[1]!.click() })
  expect(refresh).toHaveBeenCalledOnce()
})
