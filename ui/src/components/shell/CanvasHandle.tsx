// 对话 | 画布之间的分割线（B8 第二个实例；interaction §1，workbench#resize）。
// 360 到（窗口 − 侧栏 − 480）跟手；拖过下限再拖 40px 吸附关闭；拖到上限再推 40px 进入全宽。
// 单击不做任何事（防误关）；双击还原默认 46%；←→ / Shift / Home / End 键盘替代，↵ 关闭画布。
// 拖动中只改内存态预览，松手才写 `hexagon.layout.canvas`；吸附关闭 / 推入全宽时保留拖动前的宽度。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { bindingFor } from '../../keymap'
import { CANVAS, type DragResult } from '../../layout'
import { ResizeHandle } from '@/components/ui/resize-handle'

export function CanvasHandle({ width, max }: { width: number; max: number }) {
  const { t } = useTranslation()
  const st = useUiStore.getState
  const limits = { min: CANVAS.min, max, snap: CANVAS.snap, push: CANVAS.push, collapsedWidth: 0 }
  const commit = (r: DragResult, start: number) => {
    const cur = st().canvasLayout
    if (r.edge === 'snap') { st().setCanvasLayout({ ...cur, width: start }); st().setCanvasOpen(false); return }
    if (r.edge === 'full') { st().setCanvasLayout({ width: start, full: true }); return }
    st().setCanvasLayout({ width: r.width, full: false })
  }
  return (
    <ResizeHandle
      label={t('canvas.width')}
      value={width}
      limits={limits}
      direction={-1}
      tipTitle={t('canvas.close')}
      tipBinding={bindingFor('toggleCanvas')}
      tipExtra={t('layout.resetHint')}
      snapLabel={t('canvas.releaseToClose')}
      fullLabel={t('canvas.releaseToFull')}
      onDrag={(r) => { if (r.edge !== 'snap' && r.edge !== 'full') useUiStore.setState((s) => ({ canvasLayout: { ...s.canvasLayout, width: r.width } })) }}
      onCommit={commit}
      onDoubleClick={() => st().setCanvasLayout({ width: null, full: false })}
      onToggle={() => st().setCanvasOpen(false)}
    />
  )
}
