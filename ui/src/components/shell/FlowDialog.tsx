// 只读流程（ADR 0069）：侧栏「流程」分组的「查看流程」与头部 ⋯ 菜单「查看流程」打开它。
// 票 02 从旧顶栏流程包 chip 原样迁出（外观在票 13 换成 A17 Dialog）；旧右栏「阶段」页签里的
// 当前交付证据（StageEvidence：例外申请、基准复核）随阶段页签撤掉一并收进这里，避免入口丢失。
// 开关状态在 store.flowDialogOpen——它同时决定 modalScope = 'flow'（裁决快捷键据此拦截）。
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../../store'
import { api, type PackDef } from '../../api'
import { QualityCommands } from '../QualityCommands'
import { StageEvidence } from '../StageEvidence'

export function FlowDialog() {
  const { t } = useTranslation()
  const open = useUiStore((s) => s.flowDialogOpen)
  const mode = useUiStore((s) => s.mode)
  const packName = useUiStore((s) => s.packName)
  const projectEpoch = useUiStore((s) => s.projectEpoch)
  const [loaded, setFlowPack] = useState<{ epoch: number; pack: PackDef | null }>({ epoch: -1, pack: null })
  // 切项目后旧流程包作废：按 epoch 过滤，而不是在 effect 里清空
  const flowPack = loaded.epoch === projectEpoch ? loaded.pack : null
  const close = () => useUiStore.getState().setFlowDialogOpen(false)

  useEffect(() => {
    if (!open || mode === 'fastpath') return
    let cancel = false
    api.currentProcessPack().then((p) => { if (!cancel) setFlowPack({ epoch: projectEpoch, pack: p }) }).catch(() => { if (!cancel) setFlowPack({ epoch: projectEpoch, pack: null }) })
    return () => { cancel = true }
  }, [open, mode, projectEpoch])
  useEffect(() => {
    if (!open) return
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape' && !e.defaultPrevented) { e.preventDefault(); close() } }
    window.addEventListener('keydown', esc)
    return () => window.removeEventListener('keydown', esc)
  }, [open])

  if (!open) return null
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t('topbar.viewFlow')}
      data-flow-dialog
      style={{
        position: 'fixed', inset: 0, zIndex: 80,
        background: 'var(--scrim)',
        display: 'flex', alignItems: 'flex-start', justifyContent: 'center', paddingTop: 64,
      }}
      onClick={close}
    >
      <div
        className="panel panel-float"
        style={{ width: 420, maxWidth: 'calc(100% - 32px)', maxHeight: '70vh', overflow: 'auto', padding: 16 }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
          <span style={{ fontWeight: 600, fontSize: 14 }}>
            {mode === 'fastpath' ? t('topbar.fastpath') : flowPack?.name ?? packName ?? t('topbar.pack')}
          </span>
          <div style={{ flex: 1 }} />
          <button className="btn" style={{ fontSize: 12 }} onClick={close}>{t('topbar.flowClose')}</button>
        </div>
        <div className="dim3" style={{ fontSize: 11, marginBottom: 12 }}>{t('topbar.flowMaintainer')}</div>
        {mode === 'fastpath' ? (
          <div style={{ fontSize: 13 }}>{t('topbar.flowFast')}</div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {(flowPack?.stages ?? []).map((s) => (
              <div key={s.name} style={{ padding: '8px 0', borderTop: '1px solid var(--border)' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <span style={{ fontSize: 13, fontWeight: 560 }}>{s.name}</span>
                  {s.stamp_point && <span className="chip amber" style={{ fontSize: 10 }}>{t('topbar.flowStamp')}</span>}
                </div>
                {s.roles.length > 0 && (
                  <div className="dim3" style={{ fontSize: 11, marginTop: 4 }}>{s.roles.join(' · ')}</div>
                )}
              </div>
            ))}
            {!flowPack && <div className="dim3" style={{ fontSize: 12 }}>{t('sidebar.noStages')}</div>}
            <QualityCommands key={projectEpoch} />
            <StageEvidence />
          </div>
        )}
      </div>
    </div>
  )
}
