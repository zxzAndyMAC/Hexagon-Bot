// 右栏「阶段」页（ADR 0069）。只读。人不再从这里拨指针。
// 竖向时间线（ui-stage-timeline）：左轨节点 + 连线按 seq 串起，
// 已过段（done/skipped）描 --ok，未到达段 --border；当前位 =
// active/waiting_stamp/interrupted/rejected 用 accent/err 节点标记。
import { useTranslation } from 'react-i18next'
import { useUiStore } from '../store'
import type { StageRow } from '../gen/StageRow'
import { Icon } from './Icon'

type State = StageRow['state']

const PASSED: ReadonlySet<State> = new Set(['done', 'skipped'])
const STATE_KEY: Record<State, string> = {
  pending: 'pending',
  active: 'active',
  done: 'done',
  skipped: 'skipped',
  waiting_stamp: 'waiting',
  rejected: 'rejected',
  interrupted: 'interrupted',
}

const ring = (border: string, bg?: string): React.CSSProperties => ({
  width: 14,
  height: 14,
  borderRadius: '50%',
  border: `1.5px solid ${border}`,
  background: bg ?? 'var(--bg-1)',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  boxSizing: 'border-box',
  flexShrink: 0,
})

function Node({ state }: { state: State }) {
  switch (state) {
    case 'done':
      return (
        <span style={ring('var(--ok)', 'var(--ok-soft)')}>
          <Icon name="check" size={8} style={{ color: 'var(--ok)' }} />
        </span>
      )
    case 'active':
      return (
        <span style={ring('var(--accent-border)')}>
          <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--accent)', animation: 'pulse-amber 1.2s infinite' }} />
        </span>
      )
    case 'waiting_stamp':
      return (
        <span style={ring('var(--accent-border)', 'var(--accent-soft)')}>
          <Icon name="stamp" size={9} style={{ color: 'var(--accent)' }} />
        </span>
      )
    case 'interrupted':
      return (
        <span style={ring('var(--accent-border)', 'var(--accent-soft)')}>
          <Icon name="warn" size={9} style={{ color: 'var(--accent)' }} />
        </span>
      )
    case 'rejected':
      return (
        <span style={ring('var(--err-border)', 'var(--err-soft)')}>
          <Icon name="close" size={8} style={{ color: 'var(--err)' }} />
        </span>
      )
    case 'skipped':
      return <span style={{ ...ring('var(--text-3)'), opacity: 0.45 }} />
    default:
      return <span style={ring('var(--border-strong)')} />
  }
}

export function StageRail() {
  const { t } = useTranslation()
  const stages = useUiStore((s) => s.stages)
  const sorted = [...stages].sort((a, b) => a.seq - b.seq)

  return (
    <div data-stage-rail style={{ padding: '10px 12px' }}>
      {sorted.length === 0 ? (
        <div className="dim3" style={{ fontSize: 11, lineHeight: 1.5 }}>{t('side.noStages')}</div>
      ) : (
        sorted.map((s, i) => {
          const current = s.state === 'active' || s.state === 'waiting_stamp' || s.state === 'interrupted' || s.state === 'rejected'
          const dim = s.state === 'pending' || s.state === 'skipped'
          const tag = s.state !== 'pending'
          return (
            <div
              key={s.run_id}
              data-stage-state={s.state}
              title={`${s.stage} · ${t(s.state === 'done' ? 'quality.historicalDoneLabel' : `stage.${STATE_KEY[s.state]}`)}`}
              style={{ display: 'flex', alignItems: 'stretch', gap: 9 }}
            >
              <div style={{ width: 16, display: 'flex', flexDirection: 'column', alignItems: 'center', flexShrink: 0 }}>
                <div style={{ width: 2, flex: 1, background: i > 0 ? (PASSED.has(sorted[i - 1].state) ? 'var(--ok)' : 'var(--border)') : 'transparent' }} />
                <Node state={s.state} />
                <div style={{ width: 2, flex: 1, background: i < sorted.length - 1 ? (PASSED.has(s.state) ? 'var(--ok)' : 'var(--border)') : 'transparent' }} />
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '1px 0 9px', minWidth: 0 }}>
                <span
                  style={{
                    fontSize: 12,
                    color: dim ? 'var(--text-3)' : 'var(--text)',
                    fontWeight: current ? 560 : 400,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {s.stage}
                </span>
                {tag && (
                  <span
                    style={{
                      fontSize: 10,
                      flexShrink: 0,
                      color: s.state === 'rejected' ? 'var(--err)' : s.state === 'skipped' ? 'var(--text-3)' : 'var(--accent)',
                    }}
                  >
                    {t(s.state === 'done' ? 'quality.historicalDoneLabel' : `stage.${STATE_KEY[s.state]}`)}
                  </span>
                )}
              </div>
            </div>
          )
        })
      )}
    </div>
  )
}
