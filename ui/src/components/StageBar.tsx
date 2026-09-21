import { useTranslation } from 'react-i18next'
import { useEffect, useState } from 'react'
import { useUiStore } from '../store'
import { PackEditor } from './PackEditor'
import { api, errText, type PackDef } from '../api'
import { Icon } from './Icon'
import { runStageOp } from '../stageops'
import { bindingFor, formatBinding } from '../keymap'

export function StageBar() {
  const { t } = useTranslation()
  const { stages, invalidate, mode, askConfirm, pushToast } = useUiStore()
  const [editingPack, setEditingPack] = useState(false)
  const [pop, setPop] = useState<'review' | 'upgrade' | null>(null)
  const [pack, setPack] = useState<PackDef | null>(null)
  const [presets, setPresets] = useState<PackDef[]>([])
  const active = stages.find((s) => s.state === 'active' || s.state === 'waiting_stamp')
  const interrupted = stages.some((s) => s.state === 'interrupted')
  const nextPending = stages.find((s) => s.state === 'pending')

  const act = async (f: () => Promise<unknown>) => { await f(); await invalidate() }

  // 票 08：popover 打开时才拉各自数据源（声明复审名单 / 预设包单）
  useEffect(() => {
    if (pop === 'review') api.packDraft().then(setPack).catch((e) => pushToast(errText(e), 'err'))
    if (pop === 'upgrade') api.presetPacks().then(setPresets).catch((e) => pushToast(errText(e), 'err'))
  }, [pop, pushToast])

  const activeReviews =
    pack?.stages.find((s) => s.name === active?.stage)?.reviews ?? []

  return (
    <div className="row-line" style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '8px 14px', overflowX: 'auto' }}>
      {stages.map((s) => (
        <span
          key={s.run_id}
          className="chip"
          style={
            s.state === 'active' || s.state === 'waiting_stamp' || s.state === 'interrupted'
              ? { color: 'var(--accent)', borderColor: 'var(--accent-border)', background: 'var(--accent-soft)' }
              : s.state === 'done'
                ? { color: 'var(--ok)' }
                : s.state === 'skipped'
                  ? { opacity: 0.45 }
                  : {}
          }
        >
          {s.state === 'done' && <Icon name="check" size={9} />}
          {s.state === 'waiting_stamp' && <><Icon name="stamp" size={9} /> {t('stage.stampPoint')} </>}
          {s.state === 'interrupted' && <><Icon name="warn" size={9} /> {t('stage.interrupted')} </>}
          {s.stage}
        </span>
      ))}
      <div style={{ flex: 1 }} />
      {active && (
        <>
          {/* ui-audit 票 03：rewind/skip 走确认层（L2 流程破坏）；
              tooltip 显示当前绑定（ADR 0051/0056 硬约定） */}
          <button
            className="btn"
            title={`${t('stage.rewind')} ${formatBinding(bindingFor('stageRewind'))}`}
            onClick={() => runStageOp('rewind')}
          >
            {t('stage.rewind')}
          </button>
          <button
            className="btn"
            title={`${t('stage.skip')} ${formatBinding(bindingFor('stageSkip'))}`}
            onClick={() => runStageOp('skip')}
          >
            {t('stage.skip')}
          </button>
          <button className="btn" onClick={() => runStageOp('pause')}>{t('stage.pause')}</button>
          {/* ui-audit-2 票 08：overrideChecks 此前只有 composer /override 文本
              入口——失败检查旁的按钮面补齐；理由必填走确认层输入框（L2 留痕） */}
          <button
            className="btn"
            title={t('stage.overrideHint')}
            onClick={() =>
              askConfirm({
                title: t('stage.overrideTitle'),
                body: t('stage.overrideBody'),
                danger: true,
                confirmLabel: t('stage.override'),
                input: { placeholder: t('stage.overrideReasonPh'), required: true },
                run: (reason) => api.overrideChecks(reason ?? '').then(() => {}).catch((e) => pushToast(errText(e), 'err')),
              })
            }
          >
            {t('stage.override')}
          </button>
          {/* 跳过评审：声明复审的 artifact_kind 逐个可跳（L2 确认分级） */}
          <span style={{ position: 'relative' }}>
            <button className="btn" onClick={() => setPop(pop === 'review' ? null : 'review')}>
              {t('stage.skipReview')}
            </button>
            {pop === 'review' && (
              <div className="panel panel-float" style={{ position: 'absolute', top: '110%', right: 0, zIndex: 40, padding: 8, minWidth: 200 }}>
                {activeReviews.length === 0 && (
                  <div className="dim3" style={{ fontSize: 11, padding: 6 }}>{t('stage.noReviews')}</div>
                )}
                {activeReviews.map((r) => (
                  <button
                    key={r.artifact_kind}
                    className="btn"
                    style={{ display: 'block', width: '100%', textAlign: 'left', border: 'none', background: 'none', fontSize: 12 }}
                    onClick={() =>
                      askConfirm({
                        title: t('stage.skipReviewTitle', { kind: r.artifact_kind }),
                        body: t('stage.skipReviewBody', { reviewer: r.reviewer }),
                        danger: true,
                        confirmLabel: t('stage.skipReview'),
                        run: () => api.skipReview(r.artifact_kind).catch((e) => pushToast(errText(e), 'err')),
                      })
                    }
                  >
                    {r.artifact_kind} <span className="dim3" style={{ fontSize: 10 }}>→ {r.reviewer}</span>
                  </button>
                ))}
              </div>
            )}
          </span>
          {active.state === 'waiting_stamp' && (
            <button
              className="btn primary"
              title={`${t('cards.stamp')} ${formatBinding(bindingFor('stageStamp'))}`}
              onClick={() => runStageOp('stamp')}
            >
              {t('cards.stamp')}
            </button>
          )}
        </>
      )}
      {!active && nextPending && !interrupted && (
        <button className="btn primary" style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }} onClick={() => act(() => api.openStage(nextPending.seq))}>
          <Icon name="chevron-right" size={11} /> {nextPending.stage}
        </button>
      )}
      {!active && !nextPending && stages.length > 0 && !interrupted && (
        <button className="btn" onClick={() => runStageOp('resume')}>{t('stage.resume')}</button>
      )}
      {/* ui-audit-2 票 08：fastpath 项目的「升级为流程包」入口——
          upgrade_to_pack 命令此前死接线。选预设包 → 确认 → pin+mode 切换。 */}
      {mode === 'fastpath' && (
        <span style={{ position: 'relative' }}>
          <button className="btn" onClick={() => setPop(pop === 'upgrade' ? null : 'upgrade')}>
            {t('stage.upgradeToPack')}
          </button>
          {pop === 'upgrade' && (
            <div className="panel panel-float" style={{ position: 'absolute', top: '110%', right: 0, zIndex: 40, padding: 8, minWidth: 200 }}>
              {presets.length === 0 && (
                <div className="dim3" style={{ fontSize: 11, padding: 6 }}>{t('stage.noPresets')}</div>
              )}
              {presets.map((p) => (
                <button
                  key={p.name}
                  className="btn"
                  style={{ display: 'block', width: '100%', textAlign: 'left', border: 'none', background: 'none', fontSize: 12 }}
                  onClick={() =>
                    askConfirm({
                      title: t('stage.upgradeTitle', { pack: p.name }),
                      body: t('stage.upgradeBody'),
                      confirmLabel: t('stage.upgradeToPack'),
                      run: () => api.upgradeToPack(p.name).catch((e) => pushToast(errText(e), 'err')),
                    })
                  }
                >
                  {p.name} <span className="dim3" style={{ fontSize: 10 }}>v{p.version}</span>
                </button>
              ))}
            </div>
          )}
        </span>
      )}
      {stages.length > 0 && (
        <button
          className="icon-btn"
          title={t('pack.editor')}
          onClick={() => setEditingPack(true)}
        >
          <Icon name="edit" size={12} />
        </button>
      )}
      {editingPack && <PackEditor onClose={() => setEditingPack(false)} />}
    </div>
  )
}
