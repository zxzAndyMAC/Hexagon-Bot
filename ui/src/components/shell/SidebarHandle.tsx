// 侧栏 | 对话之间的分割线（B8 首个实例；interaction §1，workbench#resize）。
// 200–360 跟手，拖过下限再拖 40px 吸附成 56 宽图标列；单击 = ⌘B（toggleSidebar），双击还原默认 252；
// ←→ / Shift / Home / End / ↵ 键盘替代。拖动中只改内存态预览，松手才写 `hexagon.layout.side`。
// 吸附收起时保留拖动前的宽度——再展开回到原来的宽，而不是下限。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { bindingFor } from '../../keymap'
import { DEFAULT_SIDE_LAYOUT, SIDE, type DragResult } from '../../layout'
import { ResizeHandle } from '@/components/ui/resize-handle'

const LIMITS = { min: SIDE.min, max: SIDE.max, snap: SIDE.snap, collapsedWidth: SIDE.collapsed }

const toLayout = (r: DragResult, start: number) => (r.collapsed ? { width: start, collapsed: true } : { width: r.width, collapsed: false })

/**
 * forced（票 03）：窗口过窄、侧栏被逼成图标列。此时展开会立刻又被让位规则收回，
 * 所以分割线只报读状态，不响应单击 / ↵（不去翻负责人存的收起记忆）。
 */
export function SidebarHandle({ forced = false }: { forced?: boolean }) {
  const { t } = useTranslation()
  const layout = useUiStore((s) => s.sideLayout)
  const st = useUiStore.getState
  const toggle = forced ? undefined : () => st().toggleSidebar()
  return (
    <ResizeHandle
      label={t('sidebar.width')}
      value={layout.width}
      limits={LIMITS}
      direction={1}
      collapsed={layout.collapsed || forced}
      tipTitle={t(layout.collapsed || forced ? 'sidebar.expand' : 'sidebar.hide')}
      tipBinding={bindingFor('toggleSidebar')}
      tipExtra={t('layout.resetHint')}
      snapLabel={t('sidebar.releaseToCollapse')}
      onDrag={(r, start) => useUiStore.setState({ sideLayout: toLayout(r, start) })}
      onCommit={(r, start) => st().setSideLayout(toLayout(r, start))}
      onClick={toggle}
      onDoubleClick={forced ? undefined : () => st().setSideLayout(DEFAULT_SIDE_LAYOUT)}
      onToggle={toggle}
    />
  )
}
