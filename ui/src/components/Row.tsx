// 可达性行原语（ui-audit 票 12 / P2-13）：全库 `div onClick` 的键盘可达替代。
// 出处：Launcher 最近项目/命令面板条目/节点轨等一律裸 div——Tab 进不去、
// Enter/Space 不激活、读屏器报不出角色。统一收敛到本组件：
// role + tabIndex + Enter/Space→onClick；选中态经 aria-selected 透传。
import type { CSSProperties, KeyboardEvent, ReactNode } from 'react'

export function Row({
  role = 'button',
  selected,
  onClick,
  className,
  style,
  title,
  children,
  itemRef,
  onMouseEnter,
  onAuxClick,
}: {
  role?: 'button' | 'option' | 'tab' | 'listitem' | 'treeitem'
  selected?: boolean
  onClick: (e: React.MouseEvent<HTMLDivElement>) => void
  className?: string
  style?: CSSProperties
  title?: string
  children: ReactNode
  itemRef?: (el: HTMLDivElement | null) => void
  onMouseEnter?: () => void
  onAuxClick?: (e: React.MouseEvent<HTMLDivElement>) => void
}) {
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      e.stopPropagation()
      onClick(e as unknown as React.MouseEvent<HTMLDivElement>)
    }
  }
  return (
    <div
      ref={itemRef}
      role={role}
      tabIndex={0}
      aria-selected={role === 'option' || role === 'tab' ? selected : undefined}
      className={className}
      style={style}
      title={title}
      onClick={onClick}
      onKeyDown={onKeyDown}
      onMouseEnter={onMouseEnter}
      onAuxClick={onAuxClick}
    >
      {children}
    </div>
  )
}
