// 向导第一步放行（票 13 / ADR 0067）。
//
// 放行条件是一条可用的 default 槽：绑定存在、模型名非空、指向的供应商已启用且钥匙已存。
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
  return !!(provider?.enabled && provider.key_set)
}

/// 本机端点（不需要钥匙）：设置页状态点与向导预设过滤共用一条判定（gap B13）。
export const isLocalEndpoint = (url: string): boolean =>
  /^(https?:\/\/)?(localhost|127\.0\.0\.1|\[::1\])/i.test(url)

/// 向导服务商步能列的预设（gap B13，与设置目录同一份 core provider_presets）：
/// 向导只配 default 主对话槽，且放行要求钥匙已存（见 providerStepReady）——
/// Jev（执行判定专用）与本机端点（不要钥匙）放到设置页配。
export const wizardPreset = (p: { kind: string; base_url: string }): boolean =>
  p.kind !== 'jev' && !isLocalEndpoint(p.base_url)
