import { describe, expect, it } from 'vitest'
import type { ProviderView } from './gen/ProviderView'
import type { ProvidersView } from './gen/ProvidersView'
import { isLocalEndpoint, providerKeyReady, providerStepReady } from './providerGate'

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

// owner-followups-1015 #25：与 core provider_loopback::is_loopback 同一规则（解析后主机）。
describe('本机回环端点判定（票 25）', () => {
  it('解析后主机是 localhost / 127.0.0.0/8 / [::1] 才算', () => {
    for (const url of [
      'http://localhost:11434/v1', 'https://LOCALHOST/v1', 'http://127.0.0.1:1234/v1',
      'http://127.1.2.3:8080', 'http://[::1]:11434', ' http://localhost:1234 ',
    ]) expect(isLocalEndpoint(url), url).toBe(true)
  })

  it('字面量前缀、带用户信息、非 http(s)、IPv4 映射 IPv6、尾点都不算', () => {
    for (const url of [
      'http://localhost.evil.com/v1', 'http://127.0.0.1.nip.io/v1', 'http://127.0.0.1@evil.com/v1',
      'http://u@localhost:11434', 'http://user:pw@127.0.0.1:11434', 'ftp://localhost/v1',
      'http://[::ffff:127.0.0.1]:11434', 'http://localhost./v1', 'localhost:11434', '', 'https://api.example.com/v1',
    ]) expect(isLocalEndpoint(url), url).toBe(false)
  })

  it('钥匙就绪 = 已存钥匙或本机回环端点', () => {
    expect(providerKeyReady({ key_set: false, base_url: 'http://localhost:11434/v1' })).toBe(true)
    expect(providerKeyReady({ key_set: false, base_url: 'http://localhost.evil.com/v1' })).toBe(false)
    expect(providerKeyReady({ key_set: true, base_url: 'https://api.example.com/v1' })).toBe(true)
  })

  it('default 槽指向未存钥匙的本机端点，放行；同样无钥匙的远端不放行', () => {
    const local = { ...prov('ollama', true, false), base_url: 'http://localhost:11434/v1' }
    const lookalike = { ...prov('evil', true, false), base_url: 'http://localhost.evil.com/v1' }
    expect(providerStepReady(view([local], { default: { provider_id: 'ollama', model: 'llama3' } }))).toBe(true)
    expect(providerStepReady(view([lookalike], { default: { provider_id: 'evil', model: 'llama3' } }))).toBe(false)
  })
})
