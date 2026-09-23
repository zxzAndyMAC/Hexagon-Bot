import { describe, expect, it, beforeEach, vi, afterEach } from 'vitest'
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import { RoleEditor } from './components/RoleEditor'
import { api } from './api'
import { useUiStore } from './store'
import type { AgentDetail } from './gen/AgentDetail'
import type { ProvidersView } from './gen/ProvidersView'

;(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true

async function render(node: React.ReactNode) {
  const el = document.createElement('div')
  document.body.appendChild(el)
  const root = createRoot(el)
  await act(async () => { root.render(node) })
  return { el, root }
}

// React 受控组件在 happy-dom 下要绕 value tracker——直接 .value= 赋值
// React 仍读到旧值，onChange 等于没改。native setter 绕开追踪。
function setValue(el: Element, v: string) {
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype
  Object.getOwnPropertyDescriptor(proto, 'value')!.set!.call(el, v)
  el.dispatchEvent(new Event('change', { bubbles: true }))
}

const detail = (slot: string): AgentDetail => ({
  agent_id: 'a3', role: '前端', status: 'active', model_slot: slot, custom: false,
  def: { duty: 'd', reviewer: null, model_slot: 'chat', skills: [] },
  globs: [], grants: [],
})

const pv: ProvidersView = {
  providers: [
    { id: 'openai', name: 'OpenAI', kind: 'openai', base_url: 'https://x', models: [{ id: 'gpt-5', name: null, group: '', caps: [], context_window: null, max_output: null }], enabled: true, key_set: true },
  ],
  slots: { default: { provider_id: 'openai', model: 'gpt-4' }, 'agent:a3': { provider_id: 'openai', model: 'gpt-5' } },
}

describe('RoleEditor 直选模型（ui-audit-2 票 01）', () => {
  beforeEach(() => {
    vi.spyOn(api, 'agentDetail').mockResolvedValue(detail('chat'))
    vi.spyOn(api, 'listProviders').mockResolvedValue(pv)
    vi.spyOn(api, 'setAgentGrants').mockResolvedValue(undefined)
  })
  afterEach(() => vi.restoreAllMocks())

  it('专属模式：先绑 agent:<id> 槽再指 model_slot——顺序错了会落空到 default', async () => {
    const calls: string[] = []
    vi.spyOn(api, 'setSlotBinding').mockImplementation(async (s) => { calls.push(`bind:${s}`) })
    vi.spyOn(api, 'removeSlotBinding').mockResolvedValue(undefined)
    vi.spyOn(api, 'updateAgent').mockImplementation(async (_id, p) => { calls.push(`slot:${p.model_slot}`) })
    vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue()
    const { el, root } = await render(<RoleEditor agentId="a3" onClose={() => {}} />)
    // 切到专属模式
    const modeSel = [...el.querySelectorAll('select')].find((s) =>
      [...s.options].some((o) => o.value === 'dedicated'))!
    await act(async () => setValue(modeSel, 'dedicated'))
    // 选供应商 + 填模型
    const pidSel = [...el.querySelectorAll('select')].find((s) =>
      [...s.options].some((o) => o.value === 'openai'))!
    const modelInput = el.querySelector('input[list="role-models"]')!
    await act(async () => {
      setValue(pidSel, 'openai')
      setValue(modelInput, 'gpt-5')
    })
    const saveBtn = [...el.querySelectorAll('button')].find((b) => b.className.includes('primary'))!
    await act(async () => { saveBtn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(calls).toEqual(['bind:agent:a3', 'slot:agent:a3'])
    root.unmount()
  })

  it('专属→跟随切换：removeSlotBinding 清理 + model_slot 落共享槽', async () => {
    vi.spyOn(api, 'agentDetail').mockResolvedValue(detail('agent:a3'))
    const rm = vi.spyOn(api, 'removeSlotBinding').mockResolvedValue(undefined)
    const upd = vi.spyOn(api, 'updateAgent').mockResolvedValue(undefined)
    vi.spyOn(api, 'setSlotBinding').mockResolvedValue(undefined)
    vi.spyOn(useUiStore.getState(), 'invalidate').mockResolvedValue()
    const { el, root } = await render(<RoleEditor agentId="a3" onClose={() => {}} />)
    const modeSel = [...el.querySelectorAll('select')].find((s) =>
      [...s.options].some((o) => o.value === 'follow'))!
    await act(async () => setValue(modeSel, 'follow'))
    const saveBtn = [...el.querySelectorAll('button')].find((b) => b.className.includes('primary'))!
    await act(async () => { saveBtn.dispatchEvent(new MouseEvent('click', { bubbles: true })) })
    expect(rm).toHaveBeenCalledWith('agent:a3')
    expect(upd).toHaveBeenCalledWith('a3', expect.objectContaining({ model_slot: expect.not.stringMatching(/^agent:/) }))
    root.unmount()
  })
})
