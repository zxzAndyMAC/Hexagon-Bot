import { describe, expect, it } from 'vitest'
import type { ProviderView } from './gen/ProviderView'
import type { ProvidersView } from './gen/ProvidersView'
import { providerStepReady } from './providerGate'

function prov(
  id: string,
  enabled: boolean,
  key_set: boolean,
): ProviderView {
  return {
    id, name: id, kind: 'openai', base_url: `https://${id}.example/v1`,
    models: [], enabled, key_set,
  }
}

function view(
  providers: ProviderView[],
  slots: ProvidersView['slots'],
): ProvidersView {
  return { providers, slots }
}

describe('向导第一步放行（票 13）', () => {
  const ready = prov('or', true, true)

  it('空文档不放行', () => {
    expect(providerStepReady(view([], {}))).toBe(false)
  })

  it('只有启用的供应商、没有钥匙也没有 default 槽，不放行', () => {
    expect(providerStepReady(view([prov('or', true, false)], {}))).toBe(false)
  })

  it('启用且有钥匙，但没有 default 槽，不放行', () => {
    expect(providerStepReady(view([ready], {}))).toBe(false)
  })

  it('钥匙和启用都在，但只绑了别的槽，不放行', () => {
    expect(providerStepReady(view(
      [ready],
      { chat: { provider_id: 'or', model: 'm' } },
    ))).toBe(false)
  })

  it('default 槽指向停用的供应商，不放行', () => {
    expect(providerStepReady(view(
      [prov('or', false, true)],
      { default: { provider_id: 'or', model: 'm' } },
    ))).toBe(false)
  })

  it('default 槽指向没有钥匙的供应商，不放行', () => {
    expect(providerStepReady(view(
      [prov('or', true, false)],
      { default: { provider_id: 'or', model: 'm' } },
    ))).toBe(false)
  })

  it('default 槽模型名为空，不算配好', () => {
    expect(providerStepReady(view(
      [ready],
      { default: { provider_id: 'or', model: '  ' } },
    ))).toBe(false)
  })

  it('三项拆开凑数不放行：启用的是 A、钥匙在 B、default 指向 A', () => {
    expect(providerStepReady(view(
      [prov('a', true, false), prov('b', false, true)],
      { default: { provider_id: 'a', model: 'm' } },
    ))).toBe(false)
  })

  it('另一家已就绪，但 default 指向没钥匙的供应商，不放行', () => {
    expect(providerStepReady(view(
      [ready, prov('bad', true, false)],
      { default: { provider_id: 'bad', model: 'm' } },
    ))).toBe(false)
  })

  it('default 指向不存在的供应商，不放行', () => {
    expect(providerStepReady(view(
      [ready],
      { default: { provider_id: 'missing', model: 'm' } },
    ))).toBe(false)
  })

  it('default 槽指向启用且已存钥匙的供应商，放行', () => {
    expect(providerStepReady(view(
      [prov('other', false, false), ready],
      { default: { provider_id: 'or', model: 'deepseek/deepseek-chat' } },
    ))).toBe(true)
  })
})
