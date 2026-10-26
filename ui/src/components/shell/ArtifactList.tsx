// 产物清单：侧栏「产物」在画布里打开的清单页签（components B7「清单页签」补设计，票 03；票 09 换皮）。
// 行 = 文件类型图标 + 路径 12.5 等宽 + 版本 + 状态胶囊（A4，取值同产物视图 D13）；悬停 --hover；点一行打开 `art:<path>`。
// 右键 = 共用文件菜单（A16b，owner-followups-1015 #32，2026-10-21 负责人裁决）：打开 · 插入 · 复制路径 · 在访达中显示 · 打开方式；
// 待恢复（materialization = pending_recovery，盘上还没写成）的行只给「打开」——没有盘上文件可复制 / 揭示 / 交给别的应用。
// 同一路径只列最新版本（版本链在产物视图里看）。空态 A19 小号：说明为什么空。
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { FileTypeIcon } from '../FileTypeIcon'
import { Icon } from '../Icon'
import { Row } from '../Row'
import { artifactTone } from '../canvas/artifactMeta'
import { Badge } from '@/components/ui/badge'
import { FileContextMenu, useProjectFileTarget } from '../FileMenu'
import type { ArtifactRow } from '../../gen/ArtifactRow'

export function ArtifactList() {
  const { t } = useTranslation()
  const artifacts = useUiStore((s) => s.artifacts)
  const rows = useMemo(() => {
    const latest = new Map<string, (typeof artifacts)[number]>()
    for (const a of artifacts) if ((latest.get(a.path)?.version ?? -1) < a.version) latest.set(a.path, a)
    return [...latest.values()]
  }, [artifacts])
  if (rows.length === 0) {
    return (
      <div data-artifact-list-empty className="flex flex-1 flex-col items-center justify-center px-5 py-7 text-center">
        <span className="grid size-9 place-items-center rounded-[var(--r-card)] bg-canvas text-ink-2 shadow-[inset_0_0_0_1px_var(--line)]">
          <Icon name="layers" size={16} />
        </span>
        {/* ui-audit 票 06（P2-14）：空态给一句操作引导，不留纯白板 */}
        <p className="mt-3 max-w-[280px] text-[13px] text-ink-3">{t('side.noArtifacts')}</p>
      </div>
    )
  }
  return (
    <div role="list" className="min-h-0 flex-1 overflow-y-auto p-1.5">
      {rows.map((a) => <ArtifactListRow key={a.id} a={a} />)}
    </div>
  )
}

function ArtifactListRow({ a }: { a: ArtifactRow }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const open = () => openTab({ id: `art:${a.path}`, kind: 'artifact', title: a.path, path: a.path })
  const target = useProjectFileTarget(a.materialization === 'pending_recovery' ? null : a.path, { onOpen: open, artifact: true })
  return (
    <FileContextMenu {...target}>
      <Row
        role="listitem"
        title={a.path}
        className="hx-focus flex h-9 cursor-pointer items-center gap-2 rounded-[var(--r-sm)] px-2 text-ink-2 live:hover:bg-hover live:hover:text-ink"
        onClick={open}
      >
        <FileTypeIcon name={a.path.split('/').pop() ?? a.path} kind="file" size={14} />
        {/* 长路径截断，胶囊保形（flex 子项 min-width:0 + 胶囊不缩） */}
        <span className="min-w-0 flex-1 truncate font-mono text-[12.5px]">{a.path}</span>
        <span className="flex-none font-mono text-[12px] text-ink-3">{t('art.version', { n: a.version })}</span>
        <Badge variant={artifactTone(a)} className="flex-none">
          {a.materialization === 'pending_recovery' ? t('side.pendingRecovery') : t(`side.${a.status}`, a.status)}
        </Badge>
      </Row>
    </FileContextMenu>
  )
}
