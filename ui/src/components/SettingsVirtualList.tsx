import { useRef } from 'react'
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso'

/** Issue09: bounded DOM for installed skills. Navigation reaches unmounted rows;
 * their names, descriptions and switches retain the existing list presentation.
 * ui-redesign 票 11（components E2）：行是 listbox 的 option，外观由 ListDetail 传入（ld-item）。 */
export function SettingsVirtualList<T>({ items, itemKey, selected, onSelect, renderItem, itemClass, scrollParent }: {
  items: T[]
  /** ListDetail 的 listbox：虚拟列表借它滚动，高度随列表栏铺满（见下方 Virtuoso 注释）。 */
  scrollParent: HTMLElement | null
  itemKey: (item: T) => string
  selected: string | null
  onSelect: (key: string | null) => void
  renderItem: (item: T, selected: boolean) => React.ReactNode
  itemClass: (selected: boolean) => string
}) {
  const list = useRef<VirtuosoHandle>(null)
  const nodes = useRef(new Map<string, HTMLDivElement>())
  return <div onKeyDown={event => {
    const current = items.findIndex(item => itemKey(item) === selected)
    const next = event.key === 'ArrowDown' ? Math.min(items.length - 1, current + 1)
      : event.key === 'ArrowUp' ? Math.max(0, current - 1)
      : event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : null
    if (next == null || !items[next]) return
    // ListDetail 的 listbox 键盘处理只认已挂载行；虚拟列表自己处理并截住冒泡。
    event.preventDefault(); event.stopPropagation()
    const key = itemKey(items[next])
    onSelect(key)
    list.current?.scrollIntoView({ index: next, behavior: 'auto', done: () => nodes.current.get(key)?.focus() })
  }}>
    {/* 2026-10-26 负责人反馈「设置·技能列表高度不对」：原先自带 style 高 min(480, n×60) 且封顶 52vh，
        技能超过 100 个走虚拟列表时列表栏下半截空着、还套了第二层滚动。改为借 listbox 当滚动容器，
        高度跟列表栏走。被否决：height 100%——listbox 有 pb-2，会多出一层几像素的外滚动。 */}
    <Virtuoso ref={list} data={items} computeItemKey={(_index, item) => itemKey(item)}
      fixedItemHeight={60} increaseViewportBy={120} customScrollParent={scrollParent ?? undefined}
      itemContent={(_index, item) => {
        const key = itemKey(item)
        const active = key === selected
        return <div ref={node => { if (node) nodes.current.set(key, node); else nodes.current.delete(key) }}
          role="option" data-key={key} tabIndex={active || (selected == null && _index === 0) ? 0 : -1} aria-selected={active}
          onClick={() => onSelect(active ? null : key)}
          onKeyDown={event => {
            if (event.target === event.currentTarget && (event.key === 'Enter' || event.key === ' ')) {
              event.preventDefault(); event.stopPropagation(); onSelect(active ? null : key)
            }
          }}
          className={`${itemClass(active)} h-[58px] box-border`}>
          {renderItem(item, active)}
        </div>
      }} />
  </div>
}
