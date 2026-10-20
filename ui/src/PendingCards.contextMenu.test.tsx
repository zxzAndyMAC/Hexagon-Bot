// A16b「禁区」（owner-followups-1015 #08）：待决卡与裁决按钮上右键不出任何菜单——
// 没有自定义菜单（裁决动作永不进右键菜单，误点代价是一次未审副作用），WebView 自带菜单也被全局拦掉，也不触发裁决。
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import i18n from './i18n'
import { api, type PendingQuestion } from './api'
import { PendingCard } from './components/PendingCards'
import { TooltipProvider } from './components/ui/tooltip'
import { installContextMenuGuard } from './contextMenuGuard'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const card: PendingQuestion = {
  id: 'q1', kind: 'permission', agent_id: 'a0',
  payload: { tool: 'bash', input: { command: 'cargo test' }, reason: 'regression', safety_net: false },
  state: 'queued',
}

let uninstall: () => void
beforeEach(async () => {
  await i18n.changeLanguage('en')
  uninstall = installContextMenuGuard()
})
afterEach(() => {
  uninstall()
  vi.restoreAllMocks()
})

it('待决卡任何位置（含裁决按钮）右键：无菜单、系统菜单被拦、不裁决', async () => {
  const answer = vi.spyOn(api, 'answerPermission').mockResolvedValue(undefined as never)
  vi.spyOn(api, 'permissionShapeSuggestion').mockResolvedValue(null as never)
  const allow = vi.spyOn(api, 'allowProjectPermission').mockResolvedValue(undefined as never)
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => root.render(<TooltipProvider><PendingCard q={card} top /></TooltipProvider>))
  const targets = [el.firstElementChild!, ...el.querySelectorAll('button')]
  expect(targets.length).toBeGreaterThan(1)
  for (const target of targets) {
    const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 })
    await act(async () => { target.dispatchEvent(ev) })
    expect(ev.defaultPrevented).toBe(true)
    expect(document.querySelector('[data-slot="context-menu-content"]')).toBeNull()
  }
  expect(answer).not.toHaveBeenCalled()
  expect(allow).not.toHaveBeenCalled()
  await act(async () => root.unmount())
  el.remove()
})
