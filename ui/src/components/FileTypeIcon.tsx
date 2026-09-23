// 文件树/产物行的类型图标：vendored Material Icon Theme SVG（见 fileIcon.ts 头部出处）。
// 与描边 Icon 体系并存——文件图标是多色品牌/语言图形，不走 currentColor token，
// 这是编辑器惯例（VSCode/JetBrains 同理），不进 Icon.tsx 注册表。
import { fileIconAsset, type TreeKind } from './fileIcon'

const URLS = import.meta.glob('../assets/fileicons/*.svg', {
  eager: true,
  query: '?url',
  import: 'default',
}) as Record<string, string>

export function FileTypeIcon({ name, kind, open = false, size = 13, style }: {
  name: string
  kind: TreeKind
  open?: boolean
  size?: number
  style?: React.CSSProperties
}) {
  const icon = fileIconAsset(name, kind, open)
  return (
    <img
      src={URLS[`../assets/fileicons/${icon}.svg`] ?? URLS['../assets/fileicons/file.svg']}
      width={size}
      height={size}
      data-icon={icon}
      data-kind={kind}
      aria-hidden
      draggable={false}
      style={{ flexShrink: 0, display: 'inline-block', verticalAlign: 'middle', ...style }}
    />
  )
}
