import { describe, expect, it } from 'vitest'
import { BUILTIN_SLOTS, agentModelLabel, dedicatedSlot, isDedicatedSlot, modelDisplayName, resolveBinding, sharedSlots, slotLabel } from './modelpick'
import type { ProvidersView } from './gen/ProvidersView'
import en from './i18n/locales/en'

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

  // 功能缺口 C2：出图槽绑的是出图模型，不进「跟随谁」的对话模型选项。
  it('sharedSlots 排除出图槽 image', () => {
    const slots = { ...view.slots, image: { provider_id: 'p', model: 'gpt-image-1' } }
    expect(sharedSlots(slots)).not.toContain('image')
  })

  it('slotLabel：专属槽显示模型不裸示内部名；共享槽带槽名前缀', () => {
    expect(slotLabel('agent:a3', view, '专属')).toBe('专属 · claude-4')
    expect(slotLabel('code', view, '专属')).toBe('code · gpt-5')
    expect(slotLabel('unbound', view, '专属')).toBe('unbound · deepseek-chat') // 回落 default
    expect(slotLabel(null, view, '专属')).toBe('—')
    expect(slotLabel('agent:gone', view, '专属')).toBe('专属 · deepseek-chat')
  })

  // 票 16B（settings#team）：团队列表副标是角色实际用的模型展示名，不是「槽名 · 模型 id」。
  it('agentModelLabel：解析到的模型显示目录展示名；全未绑退回槽名', () => {
    const named: ProvidersView = { ...view, providers: [{ id: 'openai', name: 'OpenAI', enabled: true, models: [{ id: 'gpt-5', name: 'GPT-5' }] }] as unknown as ProvidersView['providers'] }
    expect(agentModelLabel('code', named, '专属')).toBe('GPT-5')
    expect(agentModelLabel('unbound', named, '专属')).toBe('deepseek-chat') // 回落 default，目录无展示名用 id
    expect(agentModelLabel('code', { providers: [], slots: {} }, '专属')).toBe('code')
  })

  // 守卫：BUILTIN_SLOTS 每个成员都必须有 providers.slot_<name> 文案——
  // 新增固定槽不配 key 会在绑定列表渲染出裸 key 路径（t 缺 key 回显原文）。
  it('BUILTIN_SLOTS 每个槽名都有 providers.slot_* 文案', () => {
    const providers = en.providers as Record<string, unknown>
    for (const s of BUILTIN_SLOTS) {
      expect(providers[`slot_${s}`], `missing providers.slot_${s}`).toBeTruthy()
    }
  })
})

// 功能缺口 B11（ui-redesign 2026-10-15）：消息头记录的模型 id 读时映射成目录展示名
describe('modelDisplayName', () => {
  const entry = (id: string, name: string | null) => ({ id, name, group: null, caps: [], context_window: null, max_output: null })
  const provider = (id: string, enabled: boolean, models: ReturnType<typeof entry>[]) =>
    ({ id, name: id, kind: 'openai' as const, base_url: '', key_set: true, enabled, models })
  const catalog: ProvidersView = {
    providers: [
      provider('off', false, [entry('deepseek-chat', 'Old DeepSeek')]),
      provider('or', true, [entry('deepseek-chat', 'DeepSeek V4'), entry('blank', '  ')]),
    ],
    slots: {},
  }
  it('启用的服务商优先，取非空展示名', () => {
    expect(modelDisplayName(catalog, 'deepseek-chat')).toBe('DeepSeek V4')
  })
  it('只有停用服务商有展示名时也映射', () => {
    const v: ProvidersView = { providers: [provider('off', false, [entry('x', 'X Model')])], slots: {} }
    expect(modelDisplayName(v, 'x')).toBe('X Model')
  })
  it('无目录项 / 展示名空白 / 视图未载入 → 原样显示 id', () => {
    expect(modelDisplayName(catalog, 'claude-opus-5-5')).toBe('claude-opus-5-5')
    expect(modelDisplayName(catalog, 'blank')).toBe('blank')
    expect(modelDisplayName(null, 'm')).toBe('m')
  })
})
