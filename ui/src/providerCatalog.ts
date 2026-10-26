// 内置服务商目录的列表规则（owner-followups-1015 #22，followups 第 12 项，2026-10-18 负责人裁决）。
// 目录本身（56 家快照 + 手工 14 条覆盖）在 core provider_presets / provider_catalog；这里只管
// 设置「模型服务」左列怎么排、怎么搜。
import type { ProviderPreset } from './gen/ProviderPreset'

/// 已启用的在前（user story 76），同组内保持原顺序（providers.json 的顺序）。
/// 否决的替代：按名称字母排——负责人记的是自己加的顺序，换序会让列表每次保存后跳动。
export function enabledFirst<P extends { enabled: boolean }>(providers: P[]): P[] {
  return [...providers.filter((p) => p.enabled), ...providers.filter((p) => !p.enabled)]
}

/// 搜索文本：显示名 + id（目录里不少条目的 id 才是常用叫法，如 `dashscope` 显示名是 Bailian）。
export const searchText = (p: { id: string; name: string }): string => `${p.name} ${p.id}`

/// 选中项对应的目录条目：预设本身，或 id 相同的已配置供应商（保存后的供应商 id 即预设 id）。
export function presetOf(presets: ProviderPreset[], id: string | undefined): ProviderPreset | undefined {
  return id ? presets.find((p) => p.id === id) : undefined
}
