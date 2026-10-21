// owner-followups-1015 #13（ADR 0093 后续裁决）：⌘K「推送」取基线上游远程、无上游让负责人选；
// 推送卡可「允许本项目」；Agent 终端推送画推送卡形，强推是安全网（无范围菜单）。
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { api, type PendingQuestion, type PushTargets } from './api'
import { useUiStore } from './store'
import { CommandPalette } from './components/CommandPalette'
import { PendingCard } from './components/PendingCards'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

let el: HTMLDivElement
let root: ReturnType<typeof createRoot>
let saved: ReturnType<typeof useUiStore.getState>
beforeEach(async () => {
  await i18n.changeLanguage('zh-CN')
  saved = useUiStore.getState()
  useUiStore.setState({ team: [], artifacts: [], pending: [], invalidate: async () => {} })
  localStorage.removeItem('hexagon.palette.recents')
  el = document.createElement('div')
  document.body.append(el)
  root = createRoot(el)
})
afterEach(async () => {
  await act(async () => root.unmount())
  el.remove()
  vi.restoreAllMocks()
  useUiStore.setState(saved, true)
  localStorage.removeItem('hexagon.palette.recents')
})

async function palette(targets: PushTargets, query: string) {
  vi.spyOn(api, 'pushTargets').mockResolvedValue(targets)
  await act(async () => root.render(<CommandPalette onClose={() => {}} />))
  const input = document.querySelector<HTMLInputElement>('[data-command-palette] input')!
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, query)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  return input
}

const enter = (input: HTMLInputElement) => act(async () => input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })))
const options = () => [...document.querySelectorAll('[role="option"]')].map((o) => o.textContent ?? '')

it('pushes to the baseline upstream, not a hard-coded origin', async () => {
  const request = vi.spyOn(api, 'requestPush').mockResolvedValue({ question_id: 'q1', pushed: null })
  const input = await palette({ baseline: 'main', upstream: 'backup', remotes: ['backup', 'origin'] }, '推送')
  expect(options().some((o) => o.includes('backup'))).toBe(true)
  await enter(input)
  expect(request).toHaveBeenCalledExactlyOnceWith('backup')
})

it('lets the owner choose a remote when the baseline has no upstream', async () => {
  const request = vi.spyOn(api, 'requestPush').mockResolvedValue({ question_id: 'q1', pushed: null })
  const input = await palette({ baseline: 'main', upstream: null, remotes: ['backup', 'origin'] }, 'push')
  expect(options().filter((o) => o.includes('推送到'))).toHaveLength(2)
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'backup')
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  await enter(input)
  expect(request).toHaveBeenCalledExactlyOnceWith('backup')
})

it('explains instead of pushing when the project has no remote', async () => {
  const request = vi.spyOn(api, 'requestPush')
  const toast = vi.spyOn(useUiStore.getState(), 'pushToast')
  const input = await palette({ baseline: 'main', upstream: null, remotes: [] }, '推送')
  await enter(input)
  expect(request).not.toHaveBeenCalled()
  expect(toast).toHaveBeenCalledWith(i18n.t('palette.pushNoRemote'), 'err')
})

it('a remembered target pushes at once and reports it, with no card to open', async () => {
  vi.spyOn(api, 'requestPush').mockResolvedValue({ question_id: 'q1', pushed: { remote: 'origin', baseline: 'main', output: '' } })
  const toast = vi.spyOn(useUiStore.getState(), 'pushToast')
  const open = vi.spyOn(useUiStore.getState(), 'openPendingDialog')
  const input = await palette({ baseline: 'main', upstream: 'origin', remotes: ['origin'] }, '推送')
  await enter(input)
  expect(toast).toHaveBeenCalledWith(i18n.t('palette.pushed', { remote: 'origin', branch: 'main' }), 'ok')
  expect(open).not.toHaveBeenCalled()
})

const pushCard: PendingQuestion = { id: 'q-push', kind: 'push', agent_id: null, state: 'queued',
  payload: { remote: 'origin', baseline: 'main' } }

it('the push card asks every time by default and remembers only when the owner ticks it', async () => {
  const confirm = vi.spyOn(api, 'confirmPush').mockResolvedValue({ remote: 'origin', baseline: 'main', output: '' })
  await act(async () => root.render(<PendingCard q={pushCard} top />))
  const box = el.querySelector<HTMLButtonElement>('[data-remember="push"]')!
  expect(box.getAttribute('aria-checked')).toBe('false')
  expect(el.textContent).toContain('origin · main')
  await act(async () => box.click())
  const confirmButton = [...el.querySelectorAll('button')].find((b) => b.textContent?.includes(i18n.t('cards.pushConfirm')))!
  await act(async () => confirmButton.click())
  expect(confirm).toHaveBeenCalledExactlyOnceWith('q-push', true)
})

const agentPush = (cmd: string, push: Record<string, unknown>, safety: boolean): PendingQuestion => ({
  id: `q-${safety ? 'force' : 'ordinary'}`, kind: 'permission', agent_id: 'a0', state: 'queued',
  payload: { tool: 'bash', input: { cmd }, safety_net: safety, push },
})

it('an Agent terminal push is a push card whose project scope is remote + branch', async () => {
  vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValue({ shape: 'push:origin main', generalized: true, tool: 'bash', project_id: 'p', agent_id: 'a0' })
  await act(async () => root.render(<PendingCard q={agentPush('git push origin main', { force: false, remote: 'origin', branch: 'main' }, false)} top />))
  expect(el.textContent).toContain(i18n.t('cards.pushFromTerminal'))
  expect(el.textContent).toContain(i18n.t('dock.kvBranch'))
  await act(async () => el.querySelector<HTMLButtonElement>('[aria-haspopup="menu"]')!.click())
  expect(el.textContent).toContain(i18n.t('projectPermission.action_push', { remote: 'origin', branch: 'main' }))
})

it('a force push is a safety net: warned, no remember option', async () => {
  const suggest = vi.spyOn(api, 'permissionShapeSuggestion')
  await act(async () => root.render(<PendingCard q={agentPush('git push --force origin main', { force: true, code: 'force_flag' }, true)} top />))
  expect(el.textContent).toContain(i18n.t('cards.pushForce'))
  expect(el.textContent).toContain(i18n.t('cards.pushForceWarn'))
  expect(el.querySelector('[aria-haspopup="menu"]')).toBeNull()
  expect(suggest).not.toHaveBeenCalled()
})
