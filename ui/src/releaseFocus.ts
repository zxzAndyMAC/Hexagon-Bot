// owner-followups-1015 #15：从别处打开设置·发布并定位到一个发布目标——托管报告「配置发布目标」、
// 发布确认卡的「打开设置·发布」、⌘K「发布…」没有目标时。App 收 hexagon:open-settings-section 'release'
// 进设置；ReleaseSection 首挂载时取走待定位的 id（设置页本来没开），已挂载时收 RELEASE_FOCUS_EVENT。
export const RELEASE_FOCUS_EVENT = 'hexagon:release-focus'

let pending: string | null = null

export function openReleaseSettings(targetId?: string | null) {
  pending = targetId || null
  window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: 'release' }))
  if (pending) window.dispatchEvent(new CustomEvent(RELEASE_FOCUS_EVENT, { detail: pending }))
}

/** 取走待定位的目标 id（只取一次，免得下次手动进设置·发布又跳回这一行）。 */
export function takeReleaseFocus(): string | null {
  const id = pending
  pending = null
  return id
}
