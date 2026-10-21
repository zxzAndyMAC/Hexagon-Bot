// 服务商图标（components A13b，owner-followups-1015 #23）：品牌 SVG 原样画在 16 / 20 / 28 方框里
// （object-contain，不裁不染色——品牌图形不走 currentColor token，同 FileTypeIcon 的编辑器惯例）；
// 深浅两版同时挂，按 html[data-theme] 由 dark: 变体切显隐，换主题不重渲染、不闪。
// 无图标（icon_key 空 / 资产缺失 / 自定义供应商）= 中性图标：`--hover-2` 底 + cpu 描边图标 ink-3，
// 不按名称或协议套品牌（providerIcons.ts 头注）。装饰性：旁边总有服务商名，故 aria-hidden。
import { providerIconAsset } from '../providerIcons'
import { Icon } from './Icon'

const BOX = {
  // 圆角只用于中性图标的底块（品牌图自带形状）：16 / 20 取组件常量 5px（foundations §3 登记），28 取 --r-sm
  16: { box: 'size-4 rounded-[5px]', glyph: 12 },
  20: { box: 'size-5 rounded-[5px]', glyph: 14 },
  28: { box: 'size-7 rounded-[var(--r-sm)]', glyph: 16 },
} as const

export type ProviderIconSize = keyof typeof BOX

export function ProviderIcon({ iconKey, size = 20, urls }: {
  iconKey: string
  size?: ProviderIconSize
  /** 只为测试注入资产表（见 providerIconAsset）。 */
  urls?: Record<string, string>
}) {
  const asset = providerIconAsset(iconKey, urls)
  const { box, glyph } = BOX[size]
  if (!asset) {
    return (
      <span data-provider-icon="neutral" aria-hidden className={`grid flex-none place-items-center bg-hover-2 text-ink-3 ${box}`}>
        {/* 内联宽高：A16 选项行的 [&_svg]:size-4 会把描边图标撑满 16 方框 */}
        <Icon name="cpu" size={glyph} style={{ width: glyph, height: glyph }} />
      </span>
    )
  }
  const img = (src: string, theme: 'light' | 'dark', cls: string) => (
    <img src={src} alt="" draggable={false} data-theme-variant={theme} className={`size-full object-contain ${cls}`} />
  )
  return (
    <span data-provider-icon={iconKey} aria-hidden className={`inline-flex flex-none ${box}`}>
      {asset.dark === asset.light
        ? img(asset.light, 'light', '')
        : <>{img(asset.light, 'light', 'dark:hidden')}{img(asset.dark, 'dark', 'hidden dark:block')}</>}
    </span>
  )
}
