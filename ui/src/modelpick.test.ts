import { describe, expect, it } from 'vitest'
import { dedicatedSlot, isDedicatedSlot, resolveBinding, sharedSlots, slotLabel } from './modelpick'
import type { ProvidersView } from './gen/ProvidersView'

const view: ProvidersView = {
  providers: [],
  slots: {
    default: { provider_id: 'openrouter', model: 'deepseek-chat' },
    code: { provider_id: 'openai', model: 'gpt-5' },
    'agent:a3': { provider_id: 'anthropic', model: 'claude-4' },
  },
}

describe('modelpick（ui-audit-2 票 01：角色直选模型）', () => {
  it('dedicatedSlot/isDedicatedSlot 互逆；共享槽名不被误判', () => {
    expect(isDedicatedSlot(dedicatedSlot('a3'))).toBe(true)
    expect(isDedicatedSlot('agent:a3')).toBe(true)
    expect(isDedicatedSlot('chat')).toBe(false)
    expect(isDedicatedSlot(null)).toBe(false)
    expect(isDedicatedSlot('agentx')).toBe(false) // 前缀必须带冒号
  })

  it('resolveBinding 镜像后端 resolve_slot：槽名未绑回落 default', () => {
    expect(resolveBinding(view.slots, 'code')?.model).toBe('gpt-5')
    expect(resolveBinding(view.slots, 'nonexistent')?.model).toBe('deepseek-chat')
    expect(resolveBinding(view.slots, null)?.model).toBe('deepseek-chat')
  })

  it('sharedSlots 排除 agent:* 专属槽且 default 恒在', () => {
    const names = sharedSlots(view.slots)
    expect(names).toContain('default')
    expect(names).toContain('code')
    expect(names).not.toContain('agent:a3')
    // 当前槽不在绑定表时（新建角色指向未绑槽）仍列得出
    expect(sharedSlots(view.slots, 'ops')).toContain('ops')
  })

  it('slotLabel：专属槽显示模型不裸示内部名；共享槽带槽名前缀', () => {
    expect(slotLabel('agent:a3', view, '专属')).toBe('专属 · claude-4')
    expect(slotLabel('code', view, '专属')).toBe('code · gpt-5')
    expect(slotLabel('unbound', view, '专属')).toBe('unbound · deepseek-chat') // 回落 default
    expect(slotLabel(null, view, '专属')).toBe('—')
    expect(slotLabel('agent:gone', view, '专属')).toBe('专属 · deepseek-chat')
  })
})
