// 可达性行原语（ui-audit 票 12 / P2-13）：全库 `div onClick` 的键盘可达替代。
// 出处：Launcher 最近项目/命令面板条目/节点轨等一律裸 div——Tab 进不去、
// Enter/Space 不激活、读屏器报不出角色。统一收敛到本组件：
// role + tabIndex + Enter/Space→onClick；选中态经 aria-selected 透传。
// owner-followups-1015 #07：其余 div 属性（ref / onContextMenu / data-* / data-state）透传，
// 好让 A16b ContextMenuTrigger asChild 直接包住行；onKeyDown 先给调用方（Shift+F10 开菜单），再做 Enter/Space。
import type { CSSProperties, DragEvent, HTMLAttributes, KeyboardEvent, ReactNode, Ref } from 'react'

export function Row({
  role = 'button',
  selected,
  onClick,
  className,
  style,
  title,
  children,
  itemRef,
  ref,
  onKeyDown: onKeyDownProp,
  draggable,
  onDragStart,
  ...rest
}: Omit<HTMLAttributes<HTMLDivElement>, 'role' | 'onClick' | 'onDragStart'> & {
  role?: 'button' | 'option' | 'tab' | 'listitem' | 'treeitem'
  selected?: boolean
  onClick: (e: React.MouseEvent<HTMLDivElement>) => void
  className?: string
  style?: CSSProperties
  title?: string
  children: ReactNode
  itemRef?: (el: HTMLDivElement | null) => void
  ref?: Ref<HTMLDivElement>
  draggable?: boolean
  onDragStart?: (e: DragEvent<HTMLDivElement>) => void
}) {
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    onKeyDownProp?.(e)
    if (e.defaultPrevented) return
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      e.stopPropagation()
      onClick(e as unknown as React.MouseEvent<HTMLDivElement>)
    }
  }
  const setRef = (el: HTMLDivElement | null) => {
    itemRef?.(el)
    if (typeof ref === 'function') ref(el)
    else if (ref) ref.current = el
  }
  return (
    <div
      {...rest}
      ref={setRef}
      role={role}
      tabIndex={0}
      aria-selected={role === 'option' || role === 'tab' ? selected : undefined}
      className={className}
      style={style}
      title={title}
      onClick={onClick}
      onKeyDown={onKeyDown}
      draggable={draggable}
      onDragStart={onDragStart}
    >
      {children}
    </div>
  )
}
