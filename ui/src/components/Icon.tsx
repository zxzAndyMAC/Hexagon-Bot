// 图标体系（规范 foundations §7，ui-redesign 票 01）：Lucide，24 视窗、描边 1.75、圆头圆角、stroke=currentColor。
// 语义名保留（调用处不动），字形从 2026-10 前的手绘 16 网格换成 Lucide；映射表见 foundations §7。
// 新图标一律进 ICONS 注册表起语义名；UI 里禁写 Unicode 几何字符（✕ ◆ ⚠ 等）。
// 颜色不内嵌——由调用处的语义 token / className 决定。文件类型图标例外，走 FileTypeIcon。
import {
  ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Bot, ChartColumn, Check, ChevronDown, ChevronRight, ChevronUp,
  CircleHelp, Columns2, Copy, Download, Eye, EyeOff, File, FileCode, FileCog, FileImage, FileJson,
  FileText, Flag, Folder, GitCompare, Globe, Hexagon, Inbox, JapaneseYen, Layers, Lightbulb, Link,
  List, Monitor, Moon, Package, PanelRight, Pause, Pencil, Plus, RefreshCw, Rocket, Settings, Shield,
  Stamp, Tag, Terminal, Trash2, TriangleAlert, Wrench, X, Zap, type LucideIcon,
} from 'lucide-react'
// ui-redesign 票 11 设置（foundations §7 原型名 keyboard / cpu / users / message2 / book / plug / logs / info / key / sun / moon / image / terminal / filter / external / gauge）
// cpu / external / image / terminal 已由票 14 / 主线注册，这里不重复导入
import { Book, Funnel, Gauge, Key, Keyboard, Logs, MessagesSquare, Plug, Sun, Users } from 'lucide-react'
// 票 10 启动页与向导（foundations §7 映射：sparkle / undo→rotate-ccw / message→message-circle /
// circleCheck / circleX / info / left→chevron-left / more 竖→ellipsis-vertical）
import { ChevronLeft, CircleCheck, CircleX, EllipsisVertical, Info, MessageCircle, RotateCcw, Sparkle } from 'lucide-react'
// ui-redesign 票 02 外壳（foundations §7 原型名 search / sidebar / more / clock / expand）
import { Clock, Ellipsis, Maximize2, PanelLeft, Search } from 'lucide-react'
// ui-redesign 票 14 电脑操控（foundations §7 原型名 external / image / cpu；collapse 为补设计；circle-check 票 10 已注册）
import { Cpu, ExternalLink, Image, Minimize2 } from 'lucide-react'
// ui-redesign 票 07 输入条与命令面板（foundations §7 原型名 crosshair / flow→workflow / test→test-tube；focus 为补设计：专注模式）
import { Crosshair, Focus, TestTube, Workflow } from 'lucide-react'
import { createLucideIcon } from 'lucide-react'

// 2026-10-14 负责人裁决（foundations §7 补设计）：侧栏 / 画布开关图标表状态——打开 = 对应一侧窗格实心，
// 关闭 = 只剩分隔线（panel-left / panel-right）。Lucide 无「实心窗格」款，按其 24 视窗与 1.75 描边自绘；
// 被否决：PanelLeftOpen/Close 的箭头款（箭头表「点了会怎样」而非当前状态）。
const PanelLeftFilled = createLucideIcon('panel-left-filled', [
  ['rect', { x: '3', y: '3', width: '18', height: '18', rx: '2', key: 'frame' }],
  ['rect', { x: '6', y: '6', width: '4.5', height: '12', rx: '1', fill: 'currentColor', stroke: 'none', key: 'pane' }],
])
const PanelRightFilled = createLucideIcon('panel-right-filled', [
  ['rect', { x: '3', y: '3', width: '18', height: '18', rx: '2', key: 'frame' }],
  ['rect', { x: '13.5', y: '6', width: '4.5', height: '12', rx: '1', fill: 'currentColor', stroke: 'none', key: 'pane' }],
])

const ICONS = {
  computer: Monitor,
  pause: Pause,
  // ---- 设置（票 11）：分区导航、数据去向行、主题分段、能力图标、筛选 ----
  keyboard: Keyboard,
  users: Users,
  messages: MessagesSquare,
  book: Book,
  plug: Plug,
  logs: Logs,
  key: Key,
  sun: Sun,
  moon: Moon,
  filter: Funnel,
  gauge: Gauge,
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
  // 票 05：思考折叠 ▴、执行类命令（foundations §7 原型名 up / terminal；sparkle、circle-x 见票 10 段，search 见票 02 段）
  'chevron-up': ChevronUp,
  terminal: Terminal,
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
  // ---- 票 10 启动页与向导 ----
  sparkle: Sparkle,
  undo: RotateCcw,
  message: MessageCircle,
  'circle-check': CircleCheck,
  'circle-x': CircleX,
  info: Info,
  'chevron-left': ChevronLeft,
  'more-vertical': EllipsisVertical,
  // ---- 外壳（票 02）：侧栏收起、搜索与命令、头部 ⋯、阶段 chip、查看流程 ----
  'panel-left': PanelLeft,
  'panel-left-open': PanelLeftFilled,
  'panel-right-open': PanelRightFilled,
  search: Search,
  more: Ellipsis,
  clock: Clock,
  expand: Maximize2,
  // ui-redesign 票 03 画布（foundations §7：columns → columns-2「与对话并排」；「对话」页签复用票 10 的 message）
  columns: Columns2,
  // ---- 电脑操控（票 14） ----
  external: ExternalLink,
  image: Image,
  cpu: Cpu,
  collapse: Minimize2,
  // ---- 输入条与命令面板（票 07） ----
  crosshair: Crosshair,
  flow: Workflow,
  test: TestTube,
  focus: Focus,
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
