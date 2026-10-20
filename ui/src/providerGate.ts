// 向导第一步放行（票 13 / ADR 0067）。
//
// 放行条件是一条可用的 default 槽：绑定存在、模型名非空、指向的供应商已启用且钥匙已存
// （本机回环端点免钥匙，owner-followups-1015 #25）。
// 三项拆开凑数不算——启用的是 A、钥匙在 B、default 指向停用的 C 时，后面的优化描述没有模型可用。
//
// 代价：false negative 只是在这一步再补一次；false positive 会走进没有模型的项目。偏向拦截。
// 否决的替代：三个布尔各自存在即可放行。
import type { ProvidersView } from './gen/ProvidersView'

export function providerStepReady(doc: ProvidersView): boolean {
  const binding = doc.slots.default
  const model = binding?.model?.trim() ?? ''
  if (!binding?.provider_id || !model) return false
  const provider = doc.providers.find((p) => p.id === binding.provider_id)
  return !!(provider?.enabled && providerKeyReady(provider))
}

/// 本机端点（不需要钥匙）：设置页状态点、钥匙提示与向导预设过滤共用一条判定（gap B13）。
/// owner-followups-1015 #25：与 core `provider_loopback::is_loopback` 同一规则——只认解析后的主机
/// （`localhost`、127.0.0.0/8、`[::1]`），方案须 http(s)、不带用户信息。
/// 否决的旧写法：前缀正则 `^(https?://)?(localhost|127.0.0.1|[::1])`——`localhost.evil.com`、
/// `127.0.0.1.nip.io` 也匹配，界面会对远端隐藏「缺少钥匙」，而 core 照样报 missing_credential。
/// 代价：误判为本机 = 隐藏一条真缺钥匙的提示；误判为远端 = 本机多一条可忽略的提示。偏向远端。
export function isLocalEndpoint(url: string): boolean {
  let u: URL
  try {
    u = new URL(url.trim())
  } catch {
    return false
  }
  if ((u.protocol !== 'http:' && u.protocol !== 'https:') || u.username || u.password) return false
  const h = u.hostname.toLowerCase()
  if (h === 'localhost' || h === '[::1]') return true
  // WHATWG URL 已把 IPv4 规范成点分十进制（`127.1` → `127.0.0.1`），这里只认第一段 127。
  return /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(h)
}

/// 钥匙这一项算不算就绪（票 25）：已存钥匙，或本机回环端点（不带鉴权头直连）。
/// 与 core `slot_ready` / `provider_loopback::decide` 同口径；只看钥匙，不看启用。
export const providerKeyReady = (p: { key_set: boolean; base_url: string }): boolean =>
  p.key_set || isLocalEndpoint(p.base_url)

/// 向导服务商步能列的预设（gap B13，与设置目录同一份 core provider_presets）：
/// 向导只配 default 主对话槽，且放行要求钥匙已存（见 providerStepReady）——
/// Jev（执行判定专用）与本机端点（不要钥匙）放到设置页配。
/// owner-followups-1015 #22：目录里没有公共地址的条目（GPUStack 等，base_url 为空）同样不列——
/// 向导选预设不展开 Base URL 框，选了也填不进地址；它们到设置页自填。子集规则本身不变。
export const wizardPreset = (p: { kind: string; base_url: string }): boolean =>
  p.kind !== 'jev' && p.base_url.trim() !== '' && !isLocalEndpoint(p.base_url)
