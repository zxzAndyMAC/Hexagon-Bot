// gap B3（ui-redesign 2026-10-15）：首屏建议点名角色（workbench#fresh「让产品策划写规格」）。
// 票 05 时建议卡文案不点名；现在取流程包第一阶段里第一个在队角色，读不到时回落旧文案。
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import i18n from './i18n'
import { FreshStart } from './components/FreshStart'
import { freshLead } from './freshLead'
import { api } from './api'
import { useUiStore } from './store'
import * as fill from './composerFill'
import type { TeamRow } from './gen/TeamRow'
import type { PackDef } from './gen/PackDef'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

const member = (role: string): TeamRow =>
  ({ id: `id-${role}`, role, model_slot: null, status: 'active', avatar_hash: null, model: null, duty: null, superior: null })
const pack = (roles: string[]) =>
  ({ name: '规格驱动', version: 1, stages: [{ name: '规格', roles, due: ['规格'], stamp_point: true }] }) as unknown as PackDef

async function render() {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(<FreshStart />) })
  await act(async () => {})
  return { el, root }
}

afterEach(() => {
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

describe('首屏建议点名角色（gap B3）', () => {
  it('freshLead 取第一阶段里第一个在队角色', () => {
    expect(freshLead(pack(['产品策划']), [member('产品策划')])).toEqual({ role: '产品策划', stage: '规格' })
    expect(freshLead(pack(['UX', '产品策划']), [member('产品策划')])).toEqual({ role: '产品策划', stage: '规格' })
    expect(freshLead(pack(['UX']), [member('产品策划')])).toBeNull()
    expect(freshLead(null, [member('产品策划')])).toBeNull()
  })

  it('有领头角色时标题点名，点击填入带 @ 点名的句子', async () => {
    await i18n.changeLanguage('zh-CN')
    useUiStore.setState({ team: [member('产品策划')], stages: [] })
    vi.spyOn(api, 'currentProcessPack').mockResolvedValue(pack(['产品策划']))
    const spy = vi.spyOn(fill, 'fillComposer').mockImplementation(() => {})
    const { el, root } = await render()
    const card = el.querySelector('[data-fresh-suggestion="spec"]') as HTMLButtonElement
    expect(card.textContent).toContain('让产品策划写规格')
    await act(async () => { card.click() })
    expect(spy).toHaveBeenCalledWith(expect.stringMatching(/^@产品策划 /))
    root.unmount()
    await i18n.changeLanguage('en')
  })

  it('读不到流程包时回落不点名文案', async () => {
    await i18n.changeLanguage('zh-CN')
    useUiStore.setState({ team: [member('产品策划')], stages: [] })
    vi.spyOn(api, 'currentProcessPack').mockRejectedValue(new Error('no pack'))
    const { el, root } = await render()
    expect(el.querySelector('[data-fresh-suggestion="spec"]')!.textContent).toContain('先写规格')
    root.unmount()
    await i18n.changeLanguage('en')
  })
})
