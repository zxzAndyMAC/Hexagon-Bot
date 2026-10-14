// 选区浮条 selbar（components D14「补 Selection Actions」，canvas#file）：38 高胶囊 overlay，padding 0 5 0 12：
// 「描述修改…」输入 + info「解释」+ wand「改写」+ 填入 ›。三个动作都只把选区作为引用填进输入条，不直接发
// （canvasSelection.ts 写了为什么）。浮在选区上方，贴不下时翻到下方；Esc 关闭。
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { selectionFill, sendSelectionToComposer, type SelectionAction } from '../../canvasSelection'
import type { SelectionAnchor } from './useSelection'
import { Icon } from '../Icon'
import { Button } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { bindingFor, formatBinding } from '../../keymap'

const BAR_H = 38
const GAP = 8

export function SelectionBar({ anchor, onClose }: { anchor: SelectionAnchor | null; onClose: () => void }) {
  const { t } = useTranslation()
  const bar = useRef<HTMLDivElement>(null)
  const [note, setNote] = useState('')
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null)
  const key = anchor ? `${anchor.sel.path}|${anchor.x}|${anchor.y}|${anchor.sel.text}` : ''
  const [noteKey, setNoteKey] = useState(key)
  if (noteKey !== key) { setNoteKey(key); setNote('') }
  useLayoutEffect(() => {
    if (!anchor || !bar.current) { setPos(null); return }
    const host = bar.current.offsetParent as HTMLElement | null
    const w = bar.current.offsetWidth
    const maxLeft = Math.max(GAP, (host?.clientWidth ?? w) - w - GAP)
    const above = anchor.y - BAR_H - GAP
    setPos({ left: Math.min(Math.max(GAP, anchor.x), maxLeft), top: above >= GAP ? above : anchor.bottom + GAP })
  }, [anchor])
  useEffect(() => {
    if (!anchor) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); onClose() } }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [anchor, onClose])
  const run = useCallback((action: SelectionAction) => {
    if (!anchor) return
    sendSelectionToComposer(selectionFill(action, anchor.sel, (k, o) => t(k, o), note))
    onClose()
  }, [anchor, note, onClose, t])
  if (!anchor) return null
  const tip = (label: string, action: 'selectionExplain' | 'selectionRewrite') =>
    [label, formatBinding(bindingFor(action))].filter(Boolean).join(' · ')
  return (
    <div ref={bar} role="toolbar" aria-label={t('canvas.sel.label')} data-selbar
      // 按下浮条按钮不能清掉编辑器里的选区（输入框除外）
      onMouseDown={(e) => { if (!(e.target instanceof HTMLInputElement)) e.preventDefault() }}
      style={pos ?? { left: anchor.x, top: Math.max(GAP, anchor.y - BAR_H - GAP) }}
      className="absolute z-[5] flex h-[38px] items-center gap-1 rounded-full bg-surface pr-[5px] pl-3 text-[13px] shadow-[var(--shadow-overlay)]">
      <input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t('canvas.sel.describe')}
        aria-label={t('canvas.sel.describe')}
        onKeyDown={(e) => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) { e.preventDefault(); run('describe') } }}
        className="w-[170px] min-w-0 bg-transparent text-[13px] text-ink outline-none placeholder:text-ink-3" />
      <Button variant="ghost" size="xs" title={tip(t('canvas.sel.explain'), 'selectionExplain')} data-sel-explain onClick={() => run('explain')}>
        <Icon name="info" size={12} />{t('canvas.sel.explain')}
      </Button>
      <Button variant="ghost" size="xs" title={tip(t('canvas.sel.rewrite'), 'selectionRewrite')} data-sel-rewrite onClick={() => run('rewrite')}>
        <Icon name="wand" size={12} />{t('canvas.sel.rewrite')}
      </Button>
      <IconButton size="sm" label={t('canvas.sel.toComposer')} action="selectionSend" data-sel-send onClick={() => run('describe')}>
        <Icon name="chevron-right" size={12} />
      </IconButton>
    </div>
  )
}
