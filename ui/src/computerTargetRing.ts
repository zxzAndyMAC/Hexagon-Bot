// 电脑动作卡缩略上的红色目标圈（components D8「电脑动作」，control#action；ui-redesign gap D8 2026-10-17）。
//
// 坐标系：click / drag 的 x/y 是「动作所依据那次观察」的截图像素（tools/desktop.rs 工具说明；原生
// Actions.swift mapPoint 再按窗口 bounds 缩放到屏幕点）——Retina 的点/像素倍率在原生侧已经吃掉，
// 卡上只需截图像素尺寸（observation image_width / image_height，core card_screenshot 带出）。
// 缩略是 object-cover 裁进 16:10 框，所以点要经过同一裁切换算；落在裁掉的部分就不画。
//
// 代价模型：圈画错 = 负责人以为点的是 A 实际点 B，比没有圈更坏（会诱导批准）；没有圈只是少一条提示。
// 所以任何一项事实不可证（尺寸缺 / 非有限数 / 坐标越界 / 快照对不上 / 实际像素尺寸与记录不符）一律返回 null。

/** 缩略框宽高比，必须与 DesktopToolDetails 的 aspect-[16/10] 一致。 */
export const THUMB_ASPECT = 16 / 10

export type RingFacts = {
  op: unknown
  x: unknown
  y: unknown
  /** drag 的终点（动作入参 to_x / to_y，同一截图像素坐标系）。 */
  toX?: unknown
  toY?: unknown
  /** 动作入参里的 snapshot_id。 */
  snapshotId: unknown
  /** 截图引用里的 snapshot_id / 像素尺寸（core card_screenshot）。 */
  shotSnapshotId: unknown
  imageWidth: unknown
  imageHeight: unknown
}

export type TargetPoint = { left: number; top: number }
/** review R4：drag 画起点 + 终点（原型没有拖拽画法，不加连线；方向由卡上「动作」行的调用式写明）。 */
export type TargetRing = TargetPoint & { role: 'start' | 'end' }

/** 动作点在缩略框里的位置（0–1 的比例，左上为原点），不可可靠映射时 null。drag 时只算起点。 */
export function ringPoint(facts: RingFacts, aspect = THUMB_ASPECT): TargetPoint | null {
  const base = trusted(facts, aspect)
  return base && inImage(facts.x, facts.y, base) ? mapPoint(facts.x as number, facts.y as number, base) : null
}

/**
 * 全部目标圈（review R4）：click 一圈；drag 起点 + 终点两圈。`aspect` = 显示框宽高比——缩略用 16:10
 * （object-cover 裁切），放大模态按图本身的宽高比（不裁切，点按 x/w、y/h 直接落位）。
 * 任何一个坐标不可证（含 drag 终点缺失或越界）整组不画；可证但落在缩略裁掉的部分只少这一圈。
 */
export function ringPoints(facts: RingFacts, aspect = THUMB_ASPECT): TargetRing[] | null {
  const base = trusted(facts, aspect)
  if (!base || !inImage(facts.x, facts.y, base)) return null
  const points: [unknown, unknown, TargetRing['role']][] = [[facts.x, facts.y, 'start']]
  if (facts.op === 'drag') {
    if (!inImage(facts.toX, facts.toY, base)) return null
    points.push([facts.toX, facts.toY, 'end'])
  }
  const rings = points.flatMap(([x, y, role]) => {
    const point = mapPoint(x as number, y as number, base)
    return point ? [{ ...point, role }] : []
  })
  return rings.length > 0 ? rings : null
}

type Base = { w: number; h: number; aspect: number }

function trusted(facts: RingFacts, aspect: number): Base | null {
  if (facts.op !== 'click' && facts.op !== 'drag') return null
  if (typeof facts.snapshotId !== 'string' || !facts.snapshotId || facts.snapshotId !== facts.shotSnapshotId) return null
  const { imageWidth: w, imageHeight: h } = facts
  if (!isNum(w) || !isNum(h) || w <= 0 || h <= 0 || !(aspect > 0)) return null
  return { w, h, aspect }
}

/** 原生 mapPoint 同样拒绝 x >= width。 */
function inImage(x: unknown, y: unknown, { w, h }: Base): boolean {
  return isNum(x) && isNum(y) && x >= 0 && y >= 0 && x < w && y < h
}

function mapPoint(x: number, y: number, { w, h, aspect }: Base): TargetPoint | null {
  // object-cover：等比放大到盖满框，居中裁掉多出的一边。以框高为 1 计：框宽 = aspect。
  const scale = Math.max(aspect / w, 1 / h)
  const left = ((aspect - w * scale) / 2 + x * scale) / aspect
  const top = ((1 - h * scale) / 2 + y * scale) / 1
  if (left < 0 || left > 1 || top < 0 || top > 1) return null
  return { left, top }
}

/** 实际加载的像素必须就是记录尺寸的那张图（同一观察），否则换算基准不成立。 */
export function sameImageSize(natural: { width: number; height: number }, imageWidth: unknown, imageHeight: unknown): boolean {
  return natural.width > 0 && natural.width === imageWidth && natural.height === imageHeight
}

const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)

/**
 * 卡片应用行（gap D8）：载荷 app = {name, title, bundle_id}（core card_app）。显示名缺失时回退 bundle id；
 * 两者都没有就不出应用行（标题走「{角色} 想{动词}」）。title 是窗口标题，空串视为没有。
 */
export function cardApp(app: unknown): { name: string; title: string } | null {
  if (!app || typeof app !== 'object') return null
  const a = app as Record<string, unknown>
  const str = (v: unknown) => (typeof v === 'string' ? v.trim() : '')
  const name = str(a.name) || str(a.bundle_id)
  return name ? { name, title: str(a.title) } : null
}
