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
afterEach(() => { root?.unmount(); el.replaceChildren(); vi.restoreAllMocks() })

it('binds abandon to the rendered card project and disables cards without host identity', async () => {
  await i18n.changeLanguage('en')
  const abandon = vi.spyOn(api, 'abandonToolAction').mockResolvedValue(undefined)
  const invalidate = vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue(undefined)
  const card: PendingQuestion = { id: 'q-control', kind: 'recovery', agent_id: 'a0', state: 'queued',
    payload: { sub: 'tool_outcome_unknown', action_id: 'action1090', host_project_root: '/project-a' } }
  root = createRoot(el)
  await act(async () => root.render(<PendingCard q={card} top />))
  const input = el.querySelector('input')!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'Owner ends recovery')
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  const button = () => el.querySelectorAll('button')[1]!
  await act(async () => button().click())
  expect(abandon).toHaveBeenCalledWith('action1090', 'Owner ends recovery', '/project-a')
  expect(invalidate).toHaveBeenCalled()
  await act(async () => root.render(<PendingCard q={{ ...card, payload: { ...card.payload, host_project_root: undefined } }} top />))
  expect(button().disabled).toBe(true)
})
