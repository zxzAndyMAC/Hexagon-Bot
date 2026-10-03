import { afterEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { PendingCard } from './components/PendingCards'
import { api, type PendingQuestion } from './api'
import i18n from './i18n'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true
const el = document.createElement('div')
let root: ReturnType<typeof createRoot>
afterEach(() => { root?.unmount(); el.replaceChildren(); el.remove(); vi.restoreAllMocks() })
const card: PendingQuestion = { id: 'q-prefill', kind: 'permission', agent_id: 'a0', state: 'queued',
  payload: { tool: 'web_fetch', input: { url: 'https://example.com/docs/a' }, safety_net: false } }

it('prefills actual editable suggestion and creates no grant until the remember button is pressed', async () => {
  await i18n.changeLanguage('zh-CN')
  vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValue({ shape: 'https://example.com/docs/*', generalized: true, tool: 'web_fetch', project_id: 'p', agent_id: 'a0' })
  const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue(undefined)
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={card} top />) })
  const input = el.querySelector('input')!
  expect(input.value).toBe('https://example.com/docs/*')
  expect(answer).not.toHaveBeenCalled()
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'https://example.com/docs/a')
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  const remember = Array.from(el.querySelectorAll('button')).find((button) => button.textContent?.includes('记住形状'))!
  await act(async () => { remember.click() })
  expect(answer).toHaveBeenCalledWith('q-prefill', true, 'https://example.com/docs/a')
})

it('does not carry a rule into another card or offer remembering for unsupported actions', async () => {
  vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValueOnce({ shape: 'exact:npm test', generalized: false, tool: 'bash', project_id: 'p', agent_id: 'a0' }).mockResolvedValueOnce(null)
  root = createRoot(el)
  await act(async () => { root.render(<PendingCard q={card} top />) })
  expect(el.querySelector('input')?.value).toBe('exact:npm test')
  await act(async () => { root.render(<PendingCard q={{ ...card, id: 'q-next' }} top />) })
  expect(el.querySelector('input')).toBeNull()
})
