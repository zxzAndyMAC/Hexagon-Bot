// 产物清单：侧栏「产物」在画布里打开的清单页签（票 03；原「画布占位」InterimCanvas 的内容原样迁来，换皮在票 09）。
// 点一行在画布打开该产物页签（`art:<path>`）。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { FileTypeIcon } from '../FileTypeIcon'
import { Row } from '../Row'

export function ArtifactList() {
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
