// 外壳布局的纯逻辑（ui-redesign 票 02，ADR 0088；interaction §1「区域尺寸」「分割线行为」）。
// 侧栏宽度与收起状态的持久化、旧键迁移、拖动与键盘的取值规则都在这里，组件只渲染结果。
// 画布（03）、文件树、设置列表的分割线复用 resolveDrag / keyTarget，只换各自的上下限。

/** 左侧栏尺寸（workbench#resize 表）：默认 252，200–360，拖过下限再拖 40px 吸附成 56 宽图标列。 */
export const SIDE = { default: 252, min: 200, max: 360, collapsed: 56, snap: 40 } as const

/** 分割线键盘步长（interaction §1：←→ 16px，Shift 64px）。 */
export const RESIZE_STEP = 16
export const RESIZE_STEP_FAST = 64

export const SIDE_LAYOUT_KEY = 'hexagon.layout.side'
/** 旧右栏开关记忆（'0' = 收起）。ADR 0088 后右栏撤掉，读到一次就迁到 SIDE_LAYOUT_KEY 并删除。 */
export const LEGACY_RAIL_KEY = 'hexagon.rail'

export type SideLayout = { width: number; collapsed: boolean }

export const DEFAULT_SIDE_LAYOUT: SideLayout = { width: SIDE.default, collapsed: false }

type KV = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

const clamp = (n: number, min: number, max: number) => Math.min(max, Math.max(min, n))

function parseSide(raw: string | null): SideLayout | null {
  if (raw == null) return null
  try {
    const v = JSON.parse(raw) as Partial<SideLayout>
    if (typeof v !== 'object' || v == null || typeof v.width !== 'number' || !Number.isFinite(v.width)) return null
    return { width: Math.round(clamp(v.width, SIDE.min, SIDE.max)), collapsed: v.collapsed === true }
  } catch {
    return null
  }
}

/**
 * 读侧栏布局。顺序：新键 → 旧 `hexagon.rail` → 默认。
 * 迁移规则（票 02）：旧键表示「⌘B 收起过」，ADR 0088 把 ⌘B 改给左侧栏，所以 '0' 迁成侧栏收起、
 * 其他值迁成展开默认宽；写入新键后删除旧键，只迁一次。新键已存在时旧键直接删掉（新键优先）。
 * 新键内容损坏时回默认，不抛——布局记忆坏了只花一次重新拖动。
 */
export function readSideLayout(storage: KV): SideLayout {
  const legacy = storage.getItem(LEGACY_RAIL_KEY)
  const saved = parseSide(storage.getItem(SIDE_LAYOUT_KEY))
  if (saved) {
    if (legacy != null) storage.removeItem(LEGACY_RAIL_KEY)
    return saved
  }
  if (legacy != null) {
    const migrated: SideLayout = { width: SIDE.default, collapsed: legacy === '0' }
    writeSideLayout(storage, migrated)
    storage.removeItem(LEGACY_RAIL_KEY)
    return migrated
  }
  return DEFAULT_SIDE_LAYOUT
}

export function writeSideLayout(storage: KV, layout: SideLayout) {
  storage.setItem(SIDE_LAYOUT_KEY, JSON.stringify({ width: Math.round(layout.width), collapsed: layout.collapsed }))
}

/**
 * 拖动取值结果：edge 决定分割线外观（min/max 变橙 `.limit`，snap 显示「松开以收起」，
 * full 显示「松开以展开到全宽」——只有给了 push 的画布分割线会出现，票 03）。
 */
export type DragResult = { width: number; collapsed: boolean; edge: 'none' | 'min' | 'max' | 'snap' | 'full' }

/** push：拖到上限后再推这么多进入「全宽」（画布，interaction §1）。 */
export type Limits = { min: number; max: number; snap?: number; collapsedWidth?: number; push?: number }

/**
 * 指针拖到 raw 宽度时的取值（workbench#resize 原型脚本同款）：
 * 低于 min − snap 进入吸附区（收起），min 与吸附区之间停在 min，超过 max 停在 max，
 * 超过 max + push 进入全宽区（画布）。不给 snap 的分割线（文件树、设置列表）停在下限，不吸附。
 */
export function resolveDrag(raw: number, { min, max, snap, collapsedWidth = 0, push }: Limits): DragResult {
  if (snap != null && raw < min - snap) return { width: collapsedWidth, collapsed: true, edge: 'snap' }
  if (raw < min) return { width: min, collapsed: false, edge: 'min' }
  if (push != null && raw > max + push) return { width: max, collapsed: false, edge: 'full' }
  if (raw > max) return { width: max, collapsed: false, edge: 'max' }
  return { width: Math.round(raw), collapsed: false, edge: 'none' }
}

/**
 * 键盘目标宽度（WCAG 2.2 拖动替代，interaction §1）：←→ 移动分割线 16px（Shift 64px），
 * Home / End 到最小 / 最大。direction = 1 表示分割线右移变宽（侧栏），-1 表示左移变宽（画布）。
 * 不认识的键返回 null，由调用方放行。
 */
export function keyTarget(key: string, shift: boolean, width: number, min: number, max: number, direction: 1 | -1 = 1): number | null {
  const step = shift ? RESIZE_STEP_FAST : RESIZE_STEP
  if (key === 'ArrowRight') return clamp(width + step * direction, min, max)
  if (key === 'ArrowLeft') return clamp(width - step * direction, min, max)
  if (key === 'Home') return min
  if (key === 'End') return max
  return null
}

// ── 画布与主区（ui-redesign 票 03，ADR 0088 / 0090；interaction §1 §3 §6，components B6 B7） ──

/**
 * 右侧画布尺寸（workbench#resize 表）：默认主区（窗口 − 侧栏）的 46%，下限 360，
 * 上限 窗口 − 侧栏 − 480（对话主区最小 480）；拖过下限再拖 40px 吸附关闭，拖到上限再推 40px 进入全宽。
 */
export const CANVAS = { ratio: 0.46, min: 360, snap: 40, push: 40 } as const
/** 对话主区最小宽度：不可压缩，两侧让位。 */
export const CONVERSATION_MIN = 480

export const CANVAS_LAYOUT_KEY = 'hexagon.layout.canvas'

/** width = null 表示没拖过，跟随默认 46%；full = 全宽（主区视图「画布全宽」）。 */
export type CanvasLayout = { width: number | null; full: boolean }

export const DEFAULT_CANVAS_LAYOUT: CanvasLayout = { width: null, full: false }

/** 读画布布局；内容损坏回默认，不抛（同侧栏：布局记忆坏了只花一次重新拖动）。 */
export function readCanvasLayout(storage: KV): CanvasLayout {
  const raw = storage.getItem(CANVAS_LAYOUT_KEY)
  if (raw == null) return DEFAULT_CANVAS_LAYOUT
  try {
    const v = JSON.parse(raw) as Partial<CanvasLayout>
    if (typeof v !== 'object' || v == null) return DEFAULT_CANVAS_LAYOUT
    const width = typeof v.width === 'number' && Number.isFinite(v.width) ? Math.round(Math.max(CANVAS.min, v.width)) : null
    return { width, full: v.full === true }
  } catch {
    return DEFAULT_CANVAS_LAYOUT
  }
}

export function writeCanvasLayout(storage: KV, layout: CanvasLayout) {
  storage.setItem(CANVAS_LAYOUT_KEY, JSON.stringify({ width: layout.width == null ? null : Math.round(layout.width), full: layout.full }))
}

// ── 画布内文件树（ui-redesign 票 09；components D14，interaction §1「区域尺寸」：210，160–360，停在下限） ──

export const TREE = { default: 210, min: 160, max: 360 } as const
export const TREE_LAYOUT_KEY = 'hexagon.layout.tree'

/** 读文件树宽度；越界夹回、损坏回默认（同侧栏：布局记忆坏了只花一次重新拖动）。 */
export function readTreeWidth(storage: KV): number {
  const n = Number(storage.getItem(TREE_LAYOUT_KEY))
  if (storage.getItem(TREE_LAYOUT_KEY) == null || !Number.isFinite(n)) return TREE.default
  return Math.round(clamp(n, TREE.min, TREE.max))
}

export function writeTreeWidth(storage: KV, width: number) {
  storage.setItem(TREE_LAYOUT_KEY, String(Math.round(clamp(width, TREE.min, TREE.max))))
}

/** 主区视图三态（interaction §3）：对话 / 画布全宽 / 用量详情。后两种时对话与输入条一起让出主区。 */
export type MainView = 'chat' | 'canvas' | 'usage'

export function mainView(s: { canvasOpen: boolean; canvasLayout: CanvasLayout; usageView: boolean }): MainView {
  if (s.usageView) return 'usage'
  return s.canvasOpen && s.canvasLayout.full ? 'canvas' : 'chat'
}

export type ShellInput = {
  windowWidth: number
  side: SideLayout
  /** 专注模式：侧栏完全隐藏（R-06）。 */
  sideHidden: boolean
  canvasOpen: boolean
  canvas: CanvasLayout
  usage: boolean
}

type SideFit = { side: 'expanded' | 'collapsed' | 'hidden'; sideWidth: number; sideForced: boolean }

export type ShellLayout = SideFit & {
  /** none = 不渲染；side = 与对话并排；full = 占满主区；sheet = 浮在对话上（窗口过窄）。 */
  canvas: 'none' | 'side' | 'full' | 'sheet'
  canvasWidth: number
  /** 并排时画布可拖的上限（窗口 − 侧栏 − 480）。 */
  canvasMax: number
}

function storedSide(side: SideLayout, hidden: boolean): SideFit {
  if (hidden) return { side: 'hidden', sideWidth: 0, sideForced: false }
  if (side.collapsed) return { side: 'collapsed', sideWidth: SIDE.collapsed, sideForced: false }
  return { side: 'expanded', sideWidth: side.width, sideForced: false }
}

/**
 * 窗口宽度下的外壳布局（interaction §1「让位顺序」）：
 * 先压画布到 360 → 侧栏收成图标列 → 画布改为浮在对话上的 Sheet。对话主区始终 ≥ 480。
 * 被逼收起的侧栏（sideForced）只是本次渲染的结果，不改 `hexagon.layout.side`——窗口放宽后回到负责人的选择。
 */
export function resolveShell(input: ShellInput): ShellLayout {
  const { windowWidth: W, side, sideHidden, canvasOpen, canvas, usage } = input
  const stored = storedSide(side, sideHidden)
  const forced: SideFit = { side: 'collapsed', sideWidth: SIDE.collapsed, sideForced: true }
  if (!canvasOpen || usage) {
    const fit = stored.side === 'expanded' && W - stored.sideWidth < CONVERSATION_MIN ? forced : stored
    return { ...fit, canvas: 'none', canvasWidth: 0, canvasMax: 0 }
  }
  if (canvas.full) return { ...stored, canvas: 'full', canvasWidth: W - stored.sideWidth, canvasMax: W - stored.sideWidth }
  const desired = canvas.width ?? Math.round(CANVAS.ratio * (W - stored.sideWidth))
  const tries = stored.side === 'expanded' ? [stored, forced] : [stored]
  for (const s of tries) {
    const max = W - s.sideWidth - CONVERSATION_MIN
    if (max >= CANVAS.min) return { ...s, canvas: 'side', canvasWidth: clamp(desired, CANVAS.min, max), canvasMax: max }
  }
  const s = tries[tries.length - 1]
  const room = Math.max(0, W - s.sideWidth)
  return { ...s, canvas: 'sheet', canvasWidth: clamp(desired, Math.min(CANVAS.min, room), room), canvasMax: room }
}
