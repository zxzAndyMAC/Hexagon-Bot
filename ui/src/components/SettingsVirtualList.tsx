import { useRef } from 'react'
import { Virtuoso, type VirtuosoHandle } from 'react-virtuoso'

/** Issue09: bounded DOM for installed skills. Navigation reaches unmounted rows;
 * their names, descriptions and switches retain the existing list presentation. */
export function SettingsVirtualList<T>({ items, itemKey, selected, onSelect, renderItem }: {
  items: T[]
  itemKey: (item: T) => string
  selected: string | null
  onSelect: (key: string | null) => void
  renderItem: (item: T) => React.ReactNode
}) {
  const list = useRef<VirtuosoHandle>(null)
  const nodes = useRef(new Map<string, HTMLDivElement>())
  return <div onKeyDown={event => {
    const current = items.findIndex(item => itemKey(item) === selected)
    const next = event.key === 'ArrowDown' ? Math.min(items.length - 1, current + 1)
      : event.key === 'ArrowUp' ? Math.max(0, current - 1)
      : event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : null
    if (next == null || !items[next]) return
    event.preventDefault()
    const key = itemKey(items[next])
    onSelect(key)
    list.current?.scrollIntoView({ index: next, behavior: 'auto', done: () => nodes.current.get(key)?.focus() })
  }}>
    <Virtuoso ref={list} data={items} computeItemKey={(_index, item) => itemKey(item)}
      fixedItemHeight={60} increaseViewportBy={120} style={{ height: Math.min(480, items.length * 60), maxHeight: '52vh' }}
      itemContent={(_index, item) => {
        const key = itemKey(item)
        const active = key === selected
        return <div ref={node => { if (node) nodes.current.set(key, node); else nodes.current.delete(key) }}
          role="button" tabIndex={0} aria-pressed={active}
          onClick={() => onSelect(active ? null : key)}
          onKeyDown={event => {
            if (event.target === event.currentTarget && (event.key === 'Enter' || event.key === ' ')) {
              event.preventDefault(); onSelect(active ? null : key)
            }
          }}
          style={{ display: 'flex', alignItems: 'center', gap: 8, height: 60, boxSizing: 'border-box',
            padding: '9px 10px', borderRadius: 8, cursor: 'pointer',
            boxShadow: active ? 'inset 2px 0 0 var(--accent)' : 'inset 2px 0 0 transparent',
            background: active ? 'var(--accent-soft)' : undefined }}>
          {renderItem(item)}
        </div>
      }} />
  </div>
}
