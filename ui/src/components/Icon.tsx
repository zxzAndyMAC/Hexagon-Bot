// 图标体系（词典 §26）：16×16 网格、1.5px 描边、圆头圆角、stroke=currentColor。
// 新图标一律进 ICONS 注册表起语义名；UI 里禁写 Unicode 几何字符（✕ ◆ ⚠ 等）。
// 颜色不内嵌——由调用处的语义 token / className 决定。

const ICONS = {
  // ---- 导航 / 动作 ----
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  'arrow-left': <path d="M10.5 8h-7M6.5 4.5 3 8l3.5 3.5" />,
  'arrow-right': <path d="M5.5 8h7M9.5 4.5 13 8l-3.5 3.5" />,
  'arrow-down': <path d="M8 3v10M4.5 9.5 8 13l3.5-3.5" />,
  export: <path d="M8 2.5v7M4.8 6.3 8 9.5l3.2-3.2M2.7 10.7v2.1a1.33 1.33 0 0 0 1.33 1.34h7.94a1.33 1.33 0 0 0 1.33-1.34v-2.1" />,
  install: <path d="M8 2v7.3M4.9 6.1 8 9.3l3.1-3.2M3 11.5v1.6a1.5 1.5 0 0 0 1.5 1.5h7a1.5 1.5 0 0 0 1.5-1.5v-1.6M3 13.2h10" />,
  edit: <path d="M9.7 3.3 12.7 6.3 5.5 13.5l-3.5.5.5-3.5zM8.9 4.1l2 2" />,
  'chevron-down': <path d="M4 6l4 4 4-4" />,
  'chevron-right': <path d="M6 4l4 4-4 4" />,
  refresh: <path d="M14 8a6 6 0 1 1-6-6c1.68 0 3.29.67 4.5 1.83L14 5.33M14 2v3.33h-3.33" />,
  send: <path d="M14.67 1.33 7.33 8.67M14.67 1.33l-4.67 13.33-2.67-6-6-2.66 13.34-4.67z" />,
  tool: <path d="M9.8 4.2a.67.67 0 0 0 0 .93l1.07 1.07a.67.67 0 0 0 .93 0l2.51-2.51a4 4 0 0 1-5.29 5.29l-4.61 4.61a1.41 1.41 0 0 1-2-2l4.61-4.61a4 4 0 0 1 5.29-5.29l-2.51 2.51z" />,
  sleep: <path d="M8 2a4 4 0 0 0 6 6 6 6 0 1 1-6-6z" />,
  // ---- 视图种类 ----
  list: <path d="M2.75 4h.02M2.75 8h.02M2.75 12h.02M5.5 4h8M5.5 8h8M5.5 12h8" />,
  artifact: <path d="M4 1.5h5.5L13 5v9.5H4zM9.5 1.5V5H13M6.5 9h4" />,
  diff: <path d="M2.5 2.5h11v11h-11zM8 2.5v11" />,
  split: <path d="M2 3.5h4.5v9H2zM9.5 3.5H14v9H9.5z" />,
  'panel-right': <path d="M2.5 2.5h11v11h-11zM10 2.5v11" />,
  agent: <path d="M8 8a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5zM3.5 13.5c.3-2.5 2.3-3.5 4.5-3.5s4.2 1 4.5 3.5" />,
  usage: <path d="M2.5 10.5a5.5 5.5 0 0 1 11 0M8 10.5l3-3.5" />,
  bolt: <path d="M8.67 1.33 2 9.33h6l-.67 5.34 6.67-8h-6l.67-5.34z" />,
  // ---- 语义标记 ----
  warn: <path d="M8 2.5 14.5 13h-13zM8 6.8v2.7M8 11.6h.01" />,
  stamp: <path d="M8 2l5 6-5 6-5-6z" />,
  hex: <path d="M8 1.8l5.4 3.1v6.2L8 14.2l-5.4-3.1V4.9z" />,
  publish: <path d="M14 10.5v2a1.33 1.33 0 0 1-1.33 1.33H3.33A1.33 1.33 0 0 1 2 12.5v-2M8 9.8V2.7M4.7 5.9 8 2.6l3.3 3.3" />,
  flag: <path d="M3 14.5V2.5M3 3h8.5l-2 3 2 3H3" />,
  escalate: <path d="M2 8a6 6 0 1 0 6-6 6.5 6.5 0 0 0-4.5 1.83L2 5.33M2 2v3.33h3.33" />,
  check: <path d="M3 8.5l3.5 3.5L13 5" />,
  help: <path d="M8 14a6 6 0 1 0 0-12 6 6 0 0 0 0 12zM6.8 6a1.5 1.5 0 0 1 2.9.5c0 1-1.7 1.3-1.7 2.5M8 11.7h.01" />,
  // ---- 物件 ----
  yen: <path d="M4.5 2 8 7l3.5-5M8 7v7M5 9.5h6M5 12h6" />,
  folder: <path d="M2 4a1 1 0 0 1 1-1h3.2l1.6 1.6H13a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z" />,
  plus: <path d="M8 3v10M3 8h10" />,
} as const

export type IconName = keyof typeof ICONS

export function Icon({ name, size = 14, className, style }: {
  name: IconName
  size?: number
  className?: string
  style?: React.CSSProperties
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      className={className}
      style={{ flexShrink: 0, display: 'inline-block', verticalAlign: 'middle', ...style }}
    >
      {ICONS[name]}
    </svg>
  )
}
