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

/** 拖动取值结果：edge 决定分割线外观（min/max 变橙 `.limit`，snap 显示「松开以收起」）。 */
export type DragResult = { width: number; collapsed: boolean; edge: 'none' | 'min' | 'max' | 'snap' }

export type Limits = { min: number; max: number; snap?: number; collapsedWidth?: number }

/**
 * 指针拖到 raw 宽度时的取值（workbench#resize 原型脚本同款）：
 * 低于 min − snap 进入吸附区（收起），min 与吸附区之间停在 min，超过 max 停在 max。
 * 不给 snap 的分割线（文件树、设置列表）停在下限，不吸附。
 */
export function resolveDrag(raw: number, { min, max, snap, collapsedWidth = 0 }: Limits): DragResult {
  if (snap != null && raw < min - snap) return { width: collapsedWidth, collapsed: true, edge: 'snap' }
  if (raw < min) return { width: min, collapsed: false, edge: 'min' }
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
