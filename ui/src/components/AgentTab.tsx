import { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import { CodeBlock, Md } from './Md'
import { Avatar } from './Avatar'
import { Icon, type IconName } from './Icon'
import { RoleEditor } from './RoleEditor'
import { fmtTime as fmtTimeShared } from '../usage'
import { pairToolCalls, toolOutcome, type ToolOutcome, toolInputSummary, TOOL_ICON, TOOL_LABEL, EXEC_CARD_TOOLS, type ToolCall } from '../agentSteps'
import { openTurns } from '../timelineModel'
import { LoadingState } from './LoadingState'
import { ExecBody, CallStatus } from './ExecCard'

type Step = {
  id: number
  kind: string
  icon: IconName
  tone: string
  label: string
  summary?: string
  detail?: string
  ok?: ToolOutcome
  message?: string
  path?: string
  divider?: boolean
  time: string
  // exec-cards 票 02：重负载工具的调用对——展开明细借 ExecBody（同源复用）。
  execCall?: ToolCall
}

const STEP_META: Record<string, { icon: IconName; tone: string }> = {
  turn_finished: { icon: 'check', tone: 'var(--ok)' },
  turn_failed: { icon: 'warn', tone: 'var(--err)' },
  agent_activated: { icon: 'bolt', tone: 'var(--ok)' },
  agent_slept: { icon: 'sleep', tone: 'var(--text-3)' },
  consult_wakeup: { icon: 'sleep', tone: 'var(--warn)' },
  stage_finished: { icon: 'stamp', tone: 'var(--accent)' },
  artifact_delivered: { icon: 'artifact', tone: 'var(--ok)' },
  artifact_rejected: { icon: 'artifact', tone: 'var(--err)' },
  review_passed: { icon: 'check', tone: 'var(--ok)' },
  review_rejected: { icon: 'warn', tone: 'var(--err)' },
  review_skipped: { icon: 'arrow-right', tone: 'var(--text-3)' },
  flag_submitted: { icon: 'flag', tone: 'var(--flag)' },
  flag_adjudicated: { icon: 'flag', tone: 'var(--flag)' },
  permission_asked: { icon: 'help', tone: 'var(--warn)' },
  permission_allowed: { icon: 'check', tone: 'var(--ok)' },
  permission_denied: { icon: 'close', tone: 'var(--err)' },
  permission_shape_remembered: { icon: 'check', tone: 'var(--text-3)' },
  escalated: { icon: 'escalate', tone: 'var(--err)' },
  test_ran: { icon: 'check', tone: 'var(--ok)' },
  proposal_queued: { icon: 'diff', tone: 'var(--accent)' },
  proposal_reviewed: { icon: 'diff', tone: 'var(--warn)' },
  proposal_stamped: { icon: 'stamp', tone: 'var(--accent)' },
  proposal_rejected: { icon: 'diff', tone: 'var(--err)' },
  proposal_activated: { icon: 'bolt', tone: 'var(--ok)' },
  proposal_rolled_back: { icon: 'arrow-left', tone: 'var(--err)' },
  publish_requested: { icon: 'publish', tone: 'var(--warn)' },
  publish_confirmed: { icon: 'publish', tone: 'var(--ok)' },
  publish_rejected: { icon: 'publish', tone: 'var(--err)' },
  publish_failed: { icon: 'publish', tone: 'var(--err)' },
  backfill_executed: { icon: 'refresh', tone: 'var(--text-2)' },
  usage_cap_hit: { icon: 'yen', tone: 'var(--err)' },
  owner_command: { icon: 'send', tone: 'var(--accent)' },
  fastpath_dispatched: { icon: 'bolt', tone: 'var(--accent)' },
  system: { icon: 'list', tone: 'var(--text-3)' },
}

// TOOL_LABEL/TOOL_ICON 已升共享层（agentSteps.ts，beautiful-ui 票 02）——
// 那里双写点号/下划线两族工具名。
// 票 14：fmtTime 收敛到 usage.ts（Intl 本地语序）；本页要秒 → withSeconds。
const fmtTime = (iso: string) => fmtTimeShared(iso, true)



/** Agent 活动视图：头卡 + 执行链路（步骤按序，可展开看 payload）。 */
export function AgentTab({ agentId }: { agentId: string }) {
  const { t } = useTranslation()
  const { team, timeline, invalidate, openTab, pushToast } = useUiStore()
  const member = team.find((m) => m.id === agentId)
  const fileRef = useRef<HTMLInputElement>(null)
  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [editing, setEditing] = useState(false)

  const steps = useMemo(() => {
    const items = timeline.filter((it) => it.event.agent_id === agentId)
    const calls = new Map(pairToolCalls(items).map((call) => [call.called.event.id, call]))
    const evLabel = (k: string) => t(`ev.${k}`, { defaultValue: k.replace(/_/g, ' ') })
    const out: Step[] = []
    for (let i = 0; i < items.length; i++) {
      const it = items[i]
      const ev = it.event
      const p = ev.payload as Record<string, unknown>
      const time = fmtTime(ev.created_at)
      const base = { id: ev.id, kind: ev.kind, time }
      if (it.message) {
        out.push({ ...base, icon: 'agent', tone: 'var(--accent)', label: evLabel('agent_message'), message: it.message.body })
        continue
      }
      if (ev.kind === 'turn_started') {
        out.push({ ...base, icon: 'bolt', tone: 'var(--accent)', label: `${evLabel('turn_started')} · ${String(p.stage ?? '')}`, divider: true })
        continue
      }
      if (ev.kind === 'tool_called') {
        const tool = String(p.tool ?? '')
        const res = calls.get(ev.id)?.result
        const exec = EXEC_CARD_TOOLS.has(tool)
        out.push({
          ...base,
          icon: TOOL_ICON[tool] ?? 'tool',
          tone: 'var(--text-2)',
          label: t(`agent.${TOOL_LABEL[tool] ?? 'stepTool'}`, { defaultValue: tool }),
          summary: toolInputSummary(p),
          detail: JSON.stringify({ ...p, ...(res ? { result: res.event.payload } : {}) }, null, 2),
          ok: toolOutcome(res),
          path: p.path ? String(p.path) : undefined,
          execCall: exec ? { called: it, result: res } : undefined,
        })
        continue
      }
      if (ev.kind === 'tool_result') continue // 已被 tool_called 吸收
      const meta = STEP_META[ev.kind] ?? { icon: 'list' as IconName, tone: 'var(--text-3)' }
      out.push({
        ...base,
        ...meta,
        label: evLabel(ev.kind),
        summary: String(p.path ?? p.stage ?? p.flag_id ?? p.proposal_id ?? p.reason ?? p.tool ?? ''),
        detail: Object.keys(p).length ? JSON.stringify(p, null, 2) : undefined,
        path: ev.kind === 'artifact_delivered' ? String(p.path ?? '') : undefined,
      })
    }
    return out
  }, [timeline, agentId, t])

  // 忙碌 = 该 agent 有未收束回合（beautiful-ui 票 04 第二落位：
  // 头卡状态旁挂像素格，回合一收自动消失）。openTurns per-agent——
  // 并发回合交错时别家的开窗不能遮本家的忙碌态。
  const turns = useMemo(() => openTurns(timeline), [timeline])
  const busy = turns.has(agentId)
  const busyAt = turns.get(agentId)

  if (!member) return <div className="dim3" style={{ padding: 14 }}>{agentId}</div>

  const sleeping = member.status === 'sleeping'
  const toggle = (id: number) => setExpanded((s) => {
    const n = new Set(s)
    if (n.has(id)) n.delete(id); else n.add(id)
    return n
  })

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      {/* 头卡 */}
      <div className="row-line" style={{ padding: '10px 14px', display: 'flex', gap: 12, alignItems: 'center' }}>
        <button
          className="icon-btn"
          style={{ padding: 0, borderRadius: '26%', lineHeight: 0 }}
          title={t('agent.changeAvatar')}
          onClick={() => fileRef.current?.click()}
        >
          <Avatar agentId={agentId} role={member.role} size={40} />
        </button>
        <input
          ref={fileRef}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/gif"
          style={{ display: 'none' }}
          onChange={(e) => {
            const f = e.target.files?.[0]
            if (!f) return
            // ui-audit 票 04（P1-6）：>2MB 本地拒绝不发请求（data URL
            // 会膨胀 ~4/3，2MB 源图 ≈ 2.7MB payload）；上传失败走 toast。
            if (f.size > 2 * 1024 * 1024) {
              pushToast(t('agent.avatarTooBig'), 'err')
              e.target.value = ''
              return
            }
            const r = new FileReader()
            r.onload = async () => {
              try {
                await api.setAgentAvatar(agentId, String(r.result))
                await invalidate('team')
              } catch (err) {
                pushToast(errText(err), 'err')
              }
            }
            r.readAsDataURL(f)
            e.target.value = ''
          }}
        />
        <div>
          <div style={{ fontWeight: 560, fontSize: 14, display: 'flex', gap: 8, alignItems: 'center' }}>
            {member.role}
            <span className={`dot ${sleeping ? 'off' : 'on'}`} />
            <span className="dim3" style={{ fontSize: 11, fontWeight: 400 }}>
              {t(`side.${member.status}`, member.status)}
            </span>
            {busy && <LoadingState label={t('timeline.working')} startedAt={busyAt ?? undefined} />}
          </div>
          <div className="dim3 mono" style={{ fontSize: 11, marginTop: 2 }}>
            {member.model_slot || '—'}
          </div>
        </div>
        <div style={{ flex: 1 }} />
        <button
          className={`btn ${sleeping ? 'primary' : ''}`}
          onClick={async () => {
            try { await api.setAgentSleeping(agentId, !sleeping); await invalidate('team') }
            catch (err) { pushToast(errText(err), 'err') }
          }}
        >
          {sleeping ? t('agent.wake') : t('agent.sleep')}
        </button>
        <button
          className="icon-btn"
          title={t('agent.editRole')}
          onClick={() => setEditing((v) => !v)}
        >
          <Icon name="edit" size={13} />
        </button>
      </div>
      {editing && <RoleEditor agentId={agentId} onClose={() => setEditing(false)} />}
      {/* 执行链路 */}
      <div style={{ flex: 1, overflowY: 'auto', paddingBottom: 8 }}>
        <div className="dim3" style={{ padding: '6px 14px', fontSize: 10, fontWeight: 560 }}>
          {t('agent.chain')}
        </div>
        {steps.length === 0 && <div className="dim3" style={{ padding: '4px 14px' }}>{t('agent.noEvents')}</div>}
        {steps.map((s, i) => s.divider ? (
          <div key={s.id} style={{ padding: '12px 14px 4px', display: 'flex', alignItems: 'center', gap: 8 }}>
            <span style={{ color: s.tone, display: 'inline-flex' }}><Icon name={s.icon} size={10} /></span>
            <span style={{ fontWeight: 560, fontSize: 11, color: 'var(--text-2)' }}>{s.label}</span>
            <span className="dim3" style={{ fontSize: 10 }}>{s.time}</span>
            <div className="sysline" style={{ flex: 1 }} />
          </div>
        ) : (
          <div
            key={s.id}
            className="row-line"
            // beautiful-ui 票 02（Q7=B）：步骤行随 fade-up 落位，stagger 封顶 10 行
            style={{ padding: '5px 14px 5px 24px', fontSize: 12, cursor: s.detail ? 'pointer' : undefined, animation: `fade-up 300ms cubic-bezier(0.23,1,0.32,1) ${Math.min(i, 10) * 35}ms both` }}
            onClick={() => { if (s.detail) toggle(s.id) }}
          >
            <div style={{ display: 'flex', gap: 8, alignItems: 'baseline' }}>
              <span style={{ color: s.tone, display: 'inline-flex', alignSelf: 'center' }}><Icon name={s.icon} size={11} /></span>
              <span style={{ fontWeight: 510 }}>{s.label}</span>
              {s.summary && (
                <span
                  className={`tool-chip${s.path ? ' file-link' : ''}`}
                  style={{ flex: '0 1 auto', maxWidth: '55%' }}
                  title={s.path ?? undefined}
                  onClick={s.path ? (e) => { e.stopPropagation(); openTab({ id: `art:${s.path}`, kind: 'artifact', title: s.path!, path: s.path! }) } : undefined}
                >
                  {s.summary}
                </span>
              )}
              {/* 票 05 TaskRows 状态机：tool_called 无 result=在途运行环；
                  落定翻 check/X 徽标 pop-in（与 ToolChipRow 同一套）。 */}
              {s.kind === 'tool_called' && <CallStatus ok={s.ok} />}
              {s.detail && <span className="dim3" style={{ display: 'inline-flex' }}><Icon name={expanded.has(s.id) ? 'chevron-down' : 'chevron-right'} size={9} /></span>}
              <span className="dim3" style={{ marginLeft: 'auto', fontSize: 10, flexShrink: 0 }}>{s.time}</span>
            </div>
            {s.message && (
              <div className="msg-body" style={{ maxWidth: '82%', marginTop: 4 }}>
                <Md>{s.message}</Md>
              </div>
            )}
            {s.detail && expanded.has(s.id) && (
              <div style={{ margin: '4px 0 2px', padding: s.execCall ? 0 : '6px 8px', background: s.execCall ? undefined : 'var(--bg-1)', borderRadius: 6, overflowX: 'auto', fontSize: 11 }}>
                {s.execCall ? <ExecBody call={s.execCall} /> : <CodeBlock code={s.detail} lang="json" />}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}
