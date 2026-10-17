// review R2（managed#report「未完成」行内动作，原型「配置发布目标 / 查看证据」）：宿主给出下一步的种类 + 目标 id
// （core `api/managed/next.rs` 封闭表），这里只把种类接到已有入口，不从理由文本猜：
// 设置类 → App 的 hexagon:open-settings-section；定位卡 → 待决坞翻到这张卡；证据 / 兜底 → 时间线定位。
import type { ManagedNext } from './gen/ManagedNext'
import { TIMELINE_JUMP_EVENT } from './timelineJump'
import { useUiStore } from './store'

/** 待决坞翻到指定卡并展开（PendingDock 监听；App 同时关掉设置页）。detail = 卡 id。 */
export const LOCATE_PENDING_EVENT = 'hexagon:locate-pending'

/** 设置类种类 → 分区（与 core 给的 target 同值；target 只作校验，不信任未知分区）。 */
const SETTINGS_SECTIONS = new Set(['models', 'usage', 'mcp', 'skills'])

export function runManagedNext(next: ManagedNext, eventId: number) {
  const jump = () => window.dispatchEvent(new CustomEvent(TIMELINE_JUMP_EVENT, { detail: eventId }))
  switch (next.kind) {
    case 'add_key': case 'model_services': case 'usage': case 'mcp': case 'skills':
      if (!SETTINGS_SECTIONS.has(next.target)) return jump()
      window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: next.target }))
      return
    case 'open_card':
      // 托管仍开着时卡不进坞（owner_pending 排除托管事项）：坞里找不到就回时间线，不出死按钮。
      if (!useUiStore.getState().pending.some(q => q.id === next.target)) return jump()
      window.dispatchEvent(new CustomEvent(LOCATE_PENDING_EVENT, { detail: next.target }))
      return
    default:
      return jump()
  }
}
