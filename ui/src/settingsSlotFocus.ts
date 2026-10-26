// 「去设置并聚焦某个模型槽」（owner-followups-1015 票 19，规范 E3 补设计）。
// 设置页在事件到达时还没挂载 SlotsSection，所以目标槽先记在这里，
// SlotsSection 首次读回配置后取走一次（只聚焦一次，之后正常打开设置不抢焦点）。
let pendingSlot: string | null = null

/** 打开设置 › 槽位绑定（2026-10-26 从模型服务拆出），并让 `slot` 那一行滚入视区、供应商下拉拿到焦点。 */
export function openSettingsSlot(slot: string): void {
  pendingSlot = slot
  window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: 'slots' }))
}

/** 取走待聚焦的槽位（取一次即清空）。 */
export function takeSlotFocus(): string | null {
  const slot = pendingSlot
  pendingSlot = null
  return slot
}
