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
  settings: <><circle cx="8" cy="8" r="2.2" /><path d="M13.5 8a5.5 5.5 0 0 0-.12-1.2l1.5-1.15-1.35-2.35-1.78.72a5.5 5.5 0 0 0-2.06-1.19L9.4 1.15H6.6l-.29 1.88a5.5 5.5 0 0 0-2.06 1.19l-1.78-.72-1.35 2.35 1.5 1.15a5.5 5.5 0 0 0 0 2.4l-1.5 1.15 1.35 2.35 1.78-.72a5.5 5.5 0 0 0 2.06 1.19l.29 1.88h2.8l.29-1.88a5.5 5.5 0 0 0 2.06-1.19l1.78.72 1.35-2.35-1.5-1.15c.08-.4.12-.8.12-1.2z" /></>,
  web: <><circle cx="8" cy="8" r="6" /><path d="M2 8h12M8 2c-2.6 2.2-2.6 9.8 0 12M8 2c2.6 2.2 2.6 9.8 0 12" /></>,
  vision: <><path d="M2 8s2.4-4.2 6-4.2S14 8 14 8s-2.4 4.2-6 4.2S2 8 2 8z" /><circle cx="8" cy="8" r="1.9" /></>,
  // 密钥显隐二态（ui-polish-2，owner 报告 vision 恒显单态）：隐藏=闭眼（eye+斜杠），显示=睁眼
  'vision-off': <><path d="M4.6 5.1C3.4 6 2.6 7.3 2 8c0 0 2.4 4.2 6 4.2 1.2 0 2.2-.4 3.1-1M7 4.2A4.6 4.6 0 0 1 8 3.8c3.6 0 6 4.2 6 4.2a11 11 0 0 1-1.5 1.9M3.5 12.5 12.5 3.5" /></>,
  reasoning: <path d="M8 2a4.2 4.2 0 0 0-2.3 7.7c.6.45 1 1 1.05 1.8h2.5c.05-.8.4-1.35 1.05-1.8A4.2 4.2 0 0 0 8 2zM6.6 13.5h2.8M7.2 15.5h1.6" />,
  free: <><path d="M2.5 3h5.8l5.2 5.2-5.8 5.3-5.2-5.2z" /><circle cx="5.8" cy="6.3" r="1" /></>,
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
  shield: <path d="M8 1.5 13.5 3.5v4c0 3.5-2.3 5.9-5.5 7-3.2-1.1-5.5-3.5-5.5-7v-4z" />,
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
  file: <path d="M4 1.5h5.5L13 5v9.5H4zM9.5 1.5V5H13" />,
  'file-code': <><path d="M4 1.5h5.5L13 5v9.5H4zM9.5 1.5V5H13" /><path d="M6.3 8.2 4.9 9.5l1.4 1.3M9.7 8.2l1.4 1.3-1.4 1.3" /></>,
  'file-text': <><path d="M4 1.5h5.5L13 5v9.5H4zM9.5 1.5V5H13" /><path d="M6 8.2h4M6 10.6h2.8" /></>,
  'file-json': <><path d="M4 1.5h5.5L13 5v9.5H4zM9.5 1.5V5H13" /><path d="M6.3 7.3c-.8.5-.8 1.1 0 1.6.8.5.8 1.1 0 1.6M9.7 7.3c.8.5.8 1.1 0 1.6-.8.5-.8 1.1 0 1.6" /></>,
  'file-image': <><path d="M4 1.5h5.5L13 5v9.5H4zM9.5 1.5V5H13" /><circle cx="7" cy="8.3" r=".8" /><path d="M5.2 12.2 7.5 9.7l1.5 1.5 1.1-1 1.7 2" /></>,
  'file-config': <><path d="M4 1.5h5.5L13 5v9.5H4zM9.5 1.5V5H13" /><circle cx="8" cy="9.3" r="1.2" /><path d="M8 7.2v.7M8 10.7v.7M6.3 8.3l.6.3M9.1 10l.6.3M6.3 10.3l.6-.3M9.1 8.6l.6-.3" /></>,
  link: <path d="M6.6 9.4 5.4 10.6a2 2 0 0 1-2.8-2.8L3.8 6.6a2 2 0 0 1 2.8 0M9.4 6.6l1.2-1.2a2 2 0 0 1 2.8 2.8l-1.2 1.2a2 2 0 0 1-2.8 0M6.4 9.6l3.2-3.2" />,
  // 双矩形错叠（iconoir copy 借形）：前框主体 + 后框露左上沿
  copy: <><path d="M10.5 2.5h-7a1.2 1.2 0 0 0-1.2 1.2v6.8" /><rect x="5.5" y="5.5" width="8.2" height="8.2" rx="1.3" /></>,
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
