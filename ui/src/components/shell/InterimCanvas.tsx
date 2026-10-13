// 画布占位（票 02 过渡态，ADR 0088）：右栏五页签撤掉后，侧栏「产物」「项目文件」与头部画布开关打开这里。
// 内容沿用旧右栏的产物交接清单与文件树（原样），点产物 / 文件仍在中栏页签打开。
// 票 03 换成真正的画布（B7：页签栏、全宽 / 并排、分割线、`hexagon.layout.canvas`），本文件随之删除。
// 旧右栏的「团队」「阶段」进了侧栏（B2/B3），「用量」改由侧栏用量入口打开用量详情。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { FileTree } from '../FileTree'
import { FileTypeIcon } from '../FileTypeIcon'
import { Row } from '../Row'
import { Icon } from '../Icon'
import { IconButton } from '@/components/ui/icon-button'

function ArtifactList() {
  const { t } = useTranslation()
  const artifacts = useUiStore((s) => s.artifacts)
  const openTab = useUiStore((s) => s.openTab)
  return (
    <div style={{ flex: 1, overflowY: 'auto' }} role="list">
      {/* ui-audit 票 06（P2-14）：空态给一句操作引导，不留纯白板 */}
      {artifacts.length === 0 && (
        <div className="dim3" style={{ padding: '14px 12px', fontSize: 11, lineHeight: 1.6 }}>
          {t('side.noArtifacts')}
        </div>
      )}
      {artifacts.map((a) => (
        <Row
          key={a.id}
          role="listitem"
          style={{ padding: '5px 12px', display: 'flex', gap: 8, alignItems: 'baseline', cursor: 'pointer' }}
          onClick={() => openTab({ id: `art:${a.path}`, kind: 'artifact', title: a.path, path: a.path })}
        >
          {/* 长名压缩 chip 的教训：flex 子项默认可缩，无 minWidth:0
              的 nowrap 文本把标记挤成竖排——路径吃 flex:1+minWidth:0
              截断，chip flexShrink:0+nowrap 保形；title 悬浮给全名。 */}
          <FileTypeIcon name={a.path.split('/').pop() ?? a.path} kind="file" size={13} style={{ alignSelf: 'center' }} />
          <span className="mono" title={a.path} style={{ flex: 1, minWidth: 0, fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{a.path}</span>
          <span className={`chip ${a.materialization === 'pending_recovery' ? 'warn' : a.status === 'stamped' ? 'amber' : a.status === 'superseded' ? '' : 'ok'}`} style={{ fontSize: 10, marginLeft: 'auto', flexShrink: 0, whiteSpace: 'nowrap' }}>
            {a.materialization === 'pending_recovery' ? t('side.pendingRecovery') : t(`side.${a.status}`, a.status)}
          </span>
        </Row>
      ))}
    </div>
  )
}

export function InterimCanvas() {
  const { t } = useTranslation()
  const open = useUiStore((s) => s.canvasOpen)
  const pane = useUiStore((s) => s.canvasPane)
  if (!open) return null
  return (
    <aside
      data-interim-canvas={pane}
      aria-label={t(pane === 'files' ? 'sidebar.files' : 'sidebar.artifacts')}
      className="flex w-[300px] min-h-0 flex-none flex-col overflow-hidden border-l border-line bg-[var(--app-main)]"
    >
      <div className="flex h-12 flex-none items-center gap-1.5 border-b border-line-soft pr-2 pl-3.5 text-[13px] font-medium text-ink">
        <Icon name={pane === 'files' ? 'folder' : 'layers'} size={14} />
        <span className="flex-1 truncate">{t(pane === 'files' ? 'sidebar.files' : 'sidebar.artifacts')}</span>
        <IconButton size="sm" label={t('header.closeCanvas')} action="toggleCanvas" onClick={() => useUiStore.getState().setCanvasOpen(false)}>
          <Icon name="close" size={14} />
        </IconButton>
      </div>
      {pane === 'files' ? <FileTree /> : <ArtifactList />}
    </aside>
  )
}
