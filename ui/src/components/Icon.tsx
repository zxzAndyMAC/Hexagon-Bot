// 图标体系（规范 foundations §7，ui-redesign 票 01）：Lucide，24 视窗、描边 1.75、圆头圆角、stroke=currentColor。
// 语义名保留（调用处不动），字形从 2026-10 前的手绘 16 网格换成 Lucide；映射表见 foundations §7。
// 新图标一律进 ICONS 注册表起语义名；UI 里禁写 Unicode 几何字符（✕ ◆ ⚠ 等）。
// 颜色不内嵌——由调用处的语义 token / className 决定。文件类型图标例外，走 FileTypeIcon。
import {
  ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Bot, ChartColumn, Check, ChevronDown, ChevronRight,
  CircleHelp, Columns2, Copy, Download, Eye, EyeOff, File, FileCode, FileCog, FileImage, FileJson,
  FileText, Flag, Folder, GitCompare, Globe, Hexagon, Inbox, JapaneseYen, Layers, Lightbulb, Link,
  List, Monitor, Moon, Package, PanelRight, Pause, Pencil, Plus, RefreshCw, Rocket, Settings, Shield,
  Stamp, Tag, Trash2, TriangleAlert, Wrench, X, Zap, type LucideIcon,
} from 'lucide-react'
// ui-redesign 票 02 外壳（foundations §7 原型名 search / sidebar / more / clock / expand）
import { Clock, Ellipsis, Maximize2, PanelLeft, Search } from 'lucide-react'
// ui-redesign 票 14 电脑操控（foundations §7 原型名 circleCheck / external / image / cpu；collapse 为补设计）
import { CircleCheck, Cpu, ExternalLink, Image, Minimize2 } from 'lucide-react'

const ICONS = {
  computer: Monitor,
  pause: Pause,
  // ---- 导航 / 动作 ----
  close: X,
  'arrow-left': ArrowLeft,
  'arrow-right': ArrowRight,
  'arrow-up': ArrowUp,
  'arrow-down': ArrowDown,
  export: Download,
  install: Package,
  edit: Pencil,
  'chevron-down': ChevronDown,
  'chevron-right': ChevronRight,
  refresh: RefreshCw,
  // 发送 = 上箭头（composer#mention 的墨色圆钮），不再是纸飞机
  send: ArrowUp,
  tool: Wrench,
  settings: Settings,
  web: Globe,
  vision: Eye,
  // 密钥显隐二态（ui-polish-2，owner 报告 vision 恒显单态）：隐藏=闭眼，显示=睁眼
  'vision-off': EyeOff,
  reasoning: Lightbulb,
  free: Tag,
  sleep: Moon,
  trash: Trash2,
  // ---- 视图种类 ----
  list: List,
  artifact: FileText,
  diff: GitCompare,
  split: Columns2,
  'panel-right': PanelRight,
  agent: Bot,
  bot: Bot,
  usage: ChartColumn,
  'chart-column': ChartColumn,
  inbox: Inbox,
  layers: Layers,
  bolt: Zap,
  // ---- 语义标记 ----
  warn: TriangleAlert,
  shield: Shield,
  stamp: Stamp,
  hex: Hexagon,
  publish: Rocket,
  flag: Flag,
  escalate: Flag,
  check: Check,
  help: CircleHelp,
  // ---- 物件 ----
  yen: JapaneseYen,
  folder: Folder,
  plus: Plus,
  file: File,
  'file-code': FileCode,
  'file-text': FileText,
  'file-json': FileJson,
  'file-image': FileImage,
  'file-config': FileCog,
  link: Link,
  copy: Copy,
  name: Tag,
  // ---- 外壳（票 02）：侧栏收起、搜索与命令、头部 ⋯、阶段 chip、查看流程 ----
  'panel-left': PanelLeft,
  search: Search,
  more: Ellipsis,
  clock: Clock,
  expand: Maximize2,
  'circle-check': CircleCheck,
  external: ExternalLink,
  image: Image,
  cpu: Cpu,
  collapse: Minimize2,
} as const satisfies Record<string, LucideIcon>

export type IconName = keyof typeof ICONS
export const ICON_NAMES = Object.keys(ICONS) as IconName[]

export function Icon({ name, size = 14, className, style }: {
  name: IconName
  size?: number
  className?: string
  style?: React.CSSProperties
}) {
  const Glyph = ICONS[name]
  return (
    <Glyph
      size={size}
      strokeWidth={1.75}
      aria-hidden
      className={className}
      style={{ flexShrink: 0, display: 'inline-block', verticalAlign: 'middle', ...style }}
    />
  )
}
