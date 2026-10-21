// 服务商品牌图标的查找接缝（owner-followups-1015 #23，followups 第 12 项，规范 components A13b）。
// 图标本体 = Cherry Studio 9d1bf05 固定快照的 81 个 SVG，原样随包于 assets/providers/
// （manifest.json 记上游路径与 sha256；许可与商标说明见仓库根 THIRD-PARTY-NOTICES）。
//
// 查找只走「供应商 id → 预设 → icon_key → 资产文件」一条链：
// - 否决的替代：按显示名或协议（kind）猜品牌——自建网关叫「OpenAI」、走 openai 兼容协议，
//   套上 OpenAI 的 Logo 就是冒用品牌（2026-10-18 负责人裁决「不按名称或协议套品牌」）。
// - 文件名约定沿用上游 registry：`<key>-light.svg` / `<key>-dark.svg`，单色图只有 `<key>.svg`。
//   缺深色版用浅色版；icon_key 为空或找不到文件 = 中性图标（返回 null）。
import { presetOf } from './providerCatalog'
import type { ProviderPreset } from './gen/ProviderPreset'

export type ProviderIconAsset = { light: string; dark: string }

const DIR = './assets/providers/'

const URLS = import.meta.glob('./assets/providers/*.svg', {
  eager: true,
  query: '?url',
  import: 'default',
}) as Record<string, string>

/// icon_key → 深浅两版的资产 URL；`urls` 只为测试注入（键形 `./assets/providers/<file>.svg`）。
export function providerIconAsset(iconKey: string, urls: Record<string, string> = URLS): ProviderIconAsset | null {
  if (!iconKey) return null
  const at = (file: string): string | undefined => urls[`${DIR}${file}.svg`]
  const darkOnly = at(`${iconKey}-dark`)
  const light = at(`${iconKey}-light`) ?? at(iconKey) ?? darkOnly
  return light ? { light, dark: darkOnly ?? light } : null
}

/// 供应商 id（已配置供应商或预设）→ 图标键；无对应预设 = ''（自定义供应商一律中性图标）。
export const iconKeyOf = (presets: ProviderPreset[], id: string | undefined): string =>
  presetOf(presets, id)?.icon_key ?? ''
