import { ModelRecoveryStatus } from './ModelRecoveryStatus'
import { ElementReference } from './ElementReferences'
import { DesktopToolDetails } from './DesktopToolDetails'
import { createContext, memo, useCallback, useContext, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type HTMLAttributes, type MouseEvent, type ReactNode } from 'react'
import { Virtuoso } from 'react-virtuoso'
import { useTimelineWindow } from '../useTimelineWindow'
import { useTimelineNodes } from '../useTimelineNodes'
import { useTimelineViewport } from '../useTimelineViewport'
import { deriveWorkbenchStatusFromFacts, factOpenTurn } from '../timelineFacts'
import { useTranslation } from 'react-i18next'
import type { TimelineItem } from '../api'
import { api, errText, isTauri } from '../api'
import { Md, CodeBlock, ReasoningMarkdown } from './Md'
import { TIMELINE_JUMP_EVENT } from '../timelineJump'
import { useUiStore } from '../store'
import { useCopyFlash } from '../useCopyFlash'
import { buildRows, compactMarks, expandedAfterPrepend, markHighlights, nodeMarks, rowContainsEvent, rowKey, deriveWorkbenchStatus, stampedByAutonomy, isInterruptedTurn, openTurn, DECISION_KINDS, SYS_HIGH_RISK, MODEL_RECOVERY_KINDS, isManagedDecision, type Filter, type NodeMark, type Row as ModelRow } from '../timelineModel'
import { Icon } from './Icon'
import { LoadingState } from './LoadingState'
import { bindingFor, formatBinding, matches } from '../keymap'
import { pairToolCalls, toolOutcome, toolFilePath, toolInputSummary, TOOL_ICON, TOOL_LABEL, EXEC_CARD_TOOLS, type ToolCall } from '../agentSteps'
import { CallStatus, ToolExecCard } from './ExecCard'
import i18n from '../i18n'
import { fmtTime } from '../usage'
import { delegationLabel } from '../delegationLabel'
import { freshTally, jumpFace, showStickButton, TIMELINE_TAIL_PX } from '../timelineStick'
import { JumpAnnouncer, JumpToLatest } from './JumpToLatest'
import { useJumpOffset } from '../useJumpOffset'
import { NodeRail } from './NodeRail'
import { MessageContextMenu } from './MessageMenu'
import { RailEvents } from '../messageActions'
import { modelDisplayName } from '../modelpick'
import type { TFunction } from 'i18next'
import './timeline.css'
import { Avatar as Face } from './ui/avatar'
import { IconButton } from './ui/icon-button'
import { CodeChip } from './ui/code-chip'
import { Segmented, SegmentedItem } from './ui/toggle-group'
import { TooltipProvider } from './ui/tooltip'
import type { AtomOptions } from '../timelineAtoms'
import { ReturnSummaryCard, ReturnSummaryMarker } from './ReturnSummary'
import { FreshStart } from './FreshStart'
import { ArtistNotice, ARTIST_NOTICE_KIND, ARTIST_PROMPT_FAILED_KIND } from './ArtistNotice'
import { ArtistProposalCard, ARTIST_PROPOSAL_KIND } from './ArtistProposal'
import { IntakeCard } from './IntakeCard'
import { isFreshProject, pinnedSummary } from '../returnSummaryModel'
import { useDelegation } from '../useDelegation'
import { ManagedRow } from './ManagedRow'
import { ManagedReport } from './ManagedReport'
import { parseTime } from '../usage'
import { SkeletonRows } from '@/components/ui/skeleton'
import { InlineError } from '@/components/ui/inline-error'
import { keepTimelineFocus } from '../timelineFocus'

// ui-audit 票 13（P2-16）：kind/subkind 回退保留但漏 key 必须留痕——
// console.warn 一次/key，开发期漏译能被看见而不是静默吞掉。
const _warnedKeys = new Set<string>()
function trKey(t: TFunction, key: string, raw: string): string {
  if (i18n.exists(key)) return t(key)
  if (!_warnedKeys.has(key)) {
    _warnedKeys.add(key)
    console.warn(`[i18n] missing key: ${key}`)
  }
  return raw.replace(/_/g, ' ')
}

// ---- 渲染 ----

// 票 14：fmtTime 收敛到 usage.ts（Intl 本地语序）。

// Thinking 升级（beautiful-ui 票 03）：live 态 shimmer 走秒，
// live→settled 翻转成「思考用时 Ns」；历史消息无计时源，回退原标签。
// 「思考」只承载推理文本（CONTEXT.md 定名）——工具调用归 ToolChips，不混排。
export function ThinkingRow({ text, live, plan = false }: { text: string; live?: boolean; plan?: boolean }) {
  const { t } = useTranslation()
  // 2026-10-01 用户验收：生成时可读，结束默认收起；手动选择不被增量覆盖。
  const [choice, setChoice] = useState<boolean | null>(null)
  const open = choice ?? live === true
  const bodyRef = useRef<HTMLDivElement>(null)
  const followTail = useRef(true)
  const bodyId = useId()
  const [now, setNow] = useState(0)
  const [start, setStart] = useState<number | null>(null)
  const [end, setEnd] = useState<number | null>(null)
  const trimmed = text.trim()
  const hasText = trimmed !== ''
  const timed = live !== undefined
  // 渲染期派生态调整（React 认可模式，NodeRail 同款）：
  // 首个非空增量起表，live→false 冻结，reset 清空归零重起；
  // live 未传（历史消息）= 无计时源不起表，不回填假时长。
  if (timed) {
    if (hasText && start == null) setStart((s) => s ?? Date.now())
    else if (!hasText && start != null) { setStart(null); setEnd(null) }
    else if (live === false && start != null && end == null) setEnd(() => Date.now())
  }
  useEffect(() => {
    if (!live) return
    const iv = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(iv)
  }, [live])
  // 2026-10-01: follow committed Markdown, not every incoming token. Reading
  // scrollHeight before deferred content commits forced layout and missed its tail.
  const followRenderedTail = useCallback(() => {
    const body = bodyRef.current
    if (body && live && open && followTail.current) body.scrollTop = body.scrollHeight
  }, [live, open])
  if (!hasText) return null
  const endAt = end ?? (now > 0 ? now : null)
  const secs = start != null && endAt != null ? Math.max(0, (endAt - start) / 1000) : null
  const label = plan ? t(live ? 'timeline.planning' : 'timeline.plan') : live
    ? `${t('timeline.thinking')} · ${Math.floor(secs ?? 0)}s`
    : timed && secs != null
      ? t('timeline.thought', { s: secs < 1 ? '<1' : String(Math.round(secs)) })
      : t('timeline.thinking')
  return (
    <div className="thinking-row" data-thinking="" data-open={open ? '1' : '0'}>
      <button type="button" className="thinking-toggle" aria-expanded={open} aria-controls={bodyId}
        onClick={() => { setChoice(!open); followTail.current = true }}>
        {/* C6：sparkle + 「思考 6 秒」 + ▾；执行计划同形 */}
        <Icon name={plan ? 'list' : 'sparkle'} size={14} />
        <span className={live ? 'shimmer-text' : undefined}>{label}</span>
        <Icon name={open ? 'chevron-up' : 'chevron-down'} size={12} />
      </button>
      {open && <div id={bodyId} ref={bodyRef} className="thinking-full" tabIndex={0}
        role="region" aria-label={t(plan ? 'timeline.plan' : 'timeline.thinking')}
        onScroll={(e) => {
          const el = e.currentTarget
          followTail.current = el.scrollHeight - el.scrollTop - el.clientHeight < 32
        }}><ReasoningMarkdown text={trimmed} live={live} onRender={followRenderedTail} /></div>}
    </div>
  )
}

// 断网等网角标（network-resilience 票 02 → components C10）：消息头行右侧 warn 胶囊，
// 7px 橙脉冲点 +「等网 0:18」。与「回复中」同一槽位互斥切换：等网期间
// 流式增量本来就停，两个状态叠着报只会互相撒谎。
function fmtClock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000))
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

function ReconnectChip({ since }: { since: number }) {
  const { t } = useTranslation()
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const iv = setInterval(() => setNow(Date.now()), 500)
    return () => clearInterval(iv)
  }, [])
  return (
    <span data-net-waiting="" className="chip warn net-waiting">
      <span className="net-dot" aria-hidden />
      {t('timeline.netWaiting')} <span className="mono">{fmtClock(now - since)}</span>
    </span>
  )
}

/** C1 头像：30（md），自定义头像走 store.avatars，读取失败回退首字。 */
export function AgentFace({ agentId, role }: { agentId: string; role: string }) {
  const src = useUiStore((s) => s.avatars[agentId])
  return <Face agentId={agentId} name={role} size={30} src={src} />
}

// 进行中的一条回复（hands-free 票 06）：增量追加在同一段，不另起多条。
// C1：Agent 回复不加气泡；头行 名 + 状态胶囊；等网时下方补一句说明（C10）。
export function LiveReply({
  role,
  model,
  text,
  thinking,
  generating,
  netWaiting,
  avatar,
}: {
  role: string
  /// gap B2：当前模型展示名（TeamRow.model），头行 `名 · 模型`
  model?: string | null
  text: string
  thinking: string
  generating: boolean
  /// 等网进入时刻（票 NR-02）：非空 = 该 agent 在等网——气泡不切走、
  /// 已收文本保留，只把状态角标换成「等网」。
  netWaiting?: number
  avatar?: ReactNode
}) {
  const { t } = useTranslation()
  const waitingNet = generating && netWaiting != null
  return (
    <div className="tl-msg" data-live-reply="">
      {avatar}
      <div className="tl-grow">
        <div className="tl-who">
          <b>{role}</b>
          {model && <span className="t" data-who-model="">{model}</span>}
          {waitingNet && <ReconnectChip since={netWaiting} />}
          {generating && netWaiting == null && (
            <span data-generating="" className="chip info">
              {t('timeline.streaming')}
            </span>
          )}
        </div>
        {/* 思考 live = 回合在跑且正文未出（正文一到即落定计时） */}
        <ThinkingRow text={thinking} live={generating && !text} />
        {text ? <CollapsibleBody text={text} live={generating} /> : null}
        {waitingNet && text ? <div className="tl-note">{t('timeline.netNote')}</div> : null}
      </div>
    </div>
  )
}

export function StatusLine() {
  const { t } = useTranslation()
  const stages = useUiStore((s) => s.stages)
  const team = useUiStore((s) => s.team)
  const timeline = useUiStore((s) => s.timeline)
  const timelineCaughtUp = useUiStore((s) => s.timelineCaughtUp)
  const streams = useUiStore((s) => s.streams)
  const thinkings = useUiStore((s) => s.thinkings)
  const streamDone = useUiStore((s) => s.streamDone)
  const facts = useUiStore(s => s.timelineFacts)
  const projectRoot = useUiStore(s => s.projectRoot)
  const st = facts || projectRoot ? deriveWorkbenchStatusFromFacts({ stages, team, facts, streams, thinkings, streamDone })
    : deriveWorkbenchStatus({ stages, team, timeline, streams, thinkings, streamDone, timelineCaughtUp })
  if (!st.stage && !st.role && !st.tool) return null
  return (
    <div data-status-line="" className="status-line">
      {st.stage && <span>{t('timeline.statusStage', { stage: st.stage })}</span>}
      {st.role && <span>{t('timeline.statusRole', { role: st.role })}</span>}
      {st.tool && <span>{t('timeline.statusTool', { tool: st.tool })}</span>}
    </div>
  )
}

// ui-audit 票 11（P3-23）：超高消息体渐隐夹持 + 展开/收起。
// 原实现 max-height+内滚——滚到底才看得见尾巴，扫读时不知道藏了内容。
function CollapsibleBody({ text, unclamped, live, atoms }: { text: string; unclamped?: boolean; live?: boolean; atoms?: AtomOptions }) {
  const { t } = useTranslation()
  const ref = useRef<HTMLDivElement>(null)
  const [tall, setTall] = useState(false)
  const [open, setOpen] = useState(false)
  // useLayoutEffect：测高必须抢在 paint 前——useEffect 会先画一帧未夹持全文
  // 再夹，虚拟列表滚回重挂时每条长消息都闪一下（owner 反馈滚动抖动）。
  useLayoutEffect(() => {
    // 票 18：story 档全量展开，不测高不夹持
    setTall(!unclamped && (ref.current?.scrollHeight ?? 0) > 260)
  }, [text, unclamped])
  return (
    <>
      {/* live=流式生成中（票 06）：最新块 stream-in + 尾端 caret，均由 CSS 驱动 */}
      <div ref={ref} className={`msg-body${live ? ' live' : ''}${tall && !open ? ' clamped' : ''}`}>
        <Md atoms={atoms}>{text}</Md>
      </div>
      {tall && (
        <button className="msg-toggle" onClick={() => setOpen(!open)}>
          {t(open ? 'timeline.collapse' : 'timeline.expand')}
        </button>
      )}
    </>
  )
}

function SystemRow({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  // ui-audit 票 05（P1-4）：system 事件按 payload.kind 子类定名——
  // 「系统」一刀切让 invariant_violation 这类信号淹没在同名行里。
  // 词表外子类回退原始 kind 文本（自文档化，不吞新类）。
  const sub = item.event.kind === 'system' ? String(item.event.payload?.kind ?? '') : ''
  if (MODEL_RECOVERY_KINDS.includes(sub)) return <ModelRecoveryRow item={item} />
  if (sub === ARTIST_NOTICE_KIND || sub === ARTIST_PROMPT_FAILED_KIND) return <ArtistNotice item={item} />
  if (sub === ARTIST_PROPOSAL_KIND) return <ArtistProposalCard item={item} />
  if (sub === 'agent_plan' && typeof item.event.payload.text === 'string') {
    return <div className="plan-row"><ThinkingRow text={item.event.payload.text} plan /></div>
  }
  const stopped = isInterruptedTurn(item)
  const highRisk = (item.event.kind === 'turn_failed' && !stopped) || SYS_HIGH_RISK.has(sub)
  const autoStamp = item.event.kind === 'stamped' && stampedByAutonomy(item.event.payload)
  const savedPermission = item.event.kind === 'permission_allowed' && item.event.payload.execution === 'queued'
  const label = sub === 'delegation_changed' && delegationLabel(t, item) ? delegationLabel(t, item)! : sub === 'tool_execution_started' ? t('ev.permission_executing', { tool: String(item.event.payload.tool ?? '') }) : savedPermission ? t('ev.permission_saved', { tool: String(item.event.payload.tool ?? '') }) : stopped ? t('ev.turn_interrupted') : autoStamp
    ? t('ev.stamped_auto', { stage: String(item.event.payload?.stage ?? '') })
    : sub
      ? trKey(t, `sys.${sub}`, sub)
      : trKey(t, `ev.${item.event.kind}`, item.event.kind)
  const note = item.event.kind === 'stamp_rejected' ? String(item.event.payload?.note ?? '')
    : item.event.kind === 'turn_failed' && !stopped ? String(item.event.payload?.error ?? item.event.payload?.outcome ?? item.event.payload?.code ?? '') : ''
  // C4 系统行：12px ink-3、两侧细线；高风险子类（SYS_HIGH_RISK / 回合失败）文字 --red。
  return (
    <div className={`sysrow tl-sys${highRisk ? ' risk' : ''}`}>
      <span className="syslabel tl-sys-l" style={highRisk ? { color: 'var(--red-ink)' } : undefined} title={note ? `${label} · ${note}` : label}>
        {label}
        {note && <span className="dim3"> · {note}</span>}
      </span>
    </div>
  )
}

/** components D11 时间线形：消息式头（头像 + 角色 + 用途 chip）→ 状态块（票 08）。 */
function ModelRecoveryRow({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const p = item.event.payload
  const view = (p.view ?? p) as Record<string, unknown>
  const agentId = String(view.agent_id ?? item.event.agent_id ?? '')
  const member = team.find((x) => x.id === agentId)
  const purpose = String(view.purpose ?? '')
  return (
    <div className="tl-msg" style={{ margin: '8px 0' }}>
      {member && <span className="tl-face"><AgentFace agentId={member.id} role={member.role} /></span>}
      <div className="tl-grow">
        <div className="tl-who">
          <b>{member?.role ?? agentId}</b>
          {purpose && <span className="t">{t(`modelRecovery.purpose.${purpose}`, { defaultValue: purpose })}</span>}
        </div>
        <ModelRecoveryStatus id={String(p.recovery_id)} payload={p} top />
      </div>
    </div>
  )
}

/** C4 系统行的通用壳（派活、异议裁决等不走 SystemRow 的事件）。 */
function Marker({ children, risk, ...rest }: { children: ReactNode; risk?: boolean } & HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={`sysrow tl-sys${risk ? ' risk' : ''}`} {...rest}>
      <span className="syslabel tl-sys-l">{children}</span>
    </div>
  )
}

const StageHeader = memo(function StageHeader({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const artifacts = useUiStore((s) => s.artifacts)
  const p = item.event.payload
  const runId = String(p.run_id ?? '')
  const n = runId ? artifacts.filter((a) => a.stage_run_id === runId).length : 0
  // C4 阶段头：26 橙底方块 + stamp、名 13/600、产物数 chip、右侧细线延伸
  return (
    <div className="tl-stagehead">
      <span className="ic"><Icon name="stamp" size={14} /></span>
      <b>{String(p.stage ?? '')}</b>
      {n > 0 && <span className="chip">{t('timeline.stageArtifacts', { count: n })}</span>}
    </div>
  )
})

/** 复制 ibtn sm（C1/C2 悬停出现；沿用 CodeBlock 复制形制）。 */
function CopyMessage({ text }: { text: string }) {
  const { t } = useTranslation()
  const { copied, copy } = useCopyFlash()
  return (
    <IconButton size="sm" className="tl-copy" label={copied ? t('md.copied') : t('timeline.copyMessage')}
      onClick={() => { void copy(text) }}>
      <Icon name={copied ? 'check' : 'copy'} size={12} />
    </IconButton>
  )
}

/** 花名册 → 点名识别口径（同输入条：角色名与 角色[id] 两种写法）。引用稳定才不破 Md 的 memo。 */
function useAtomOptions(owner: boolean): AtomOptions {
  const team = useUiStore((s) => s.team)
  return useMemo(() => ({ owner, roles: new Set(team.flatMap((m) => [m.role, `${m.role}[${m.id}]`])) }), [team, owner])
}

// C11 交付卡：状态 → 胶囊。valid/pending/queued = 待复审 warn；退回/失败 err；通过 ok。
function deliveryChip(status: string): { tone: string; key: string } {
  if (status === 'passed' || status === 'stamped') return { tone: 'ok', key: 'timeline.deliveryPassed' }
  if (status === 'rejected' || status === 'failed') return { tone: 'err', key: 'timeline.deliveryRejected' }
  return { tone: 'warn', key: 'timeline.deliveryPending' }
}

function DeliveryCard({ item }: { item: TimelineItem }) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const p = item.event.payload
  const path = String(p.path ?? '')
  const version = Number(p.version ?? 0)
  // 已被同路径更高版本取代：opacity .6 + neutral chip（C11 禁用态）
  const superseded = useUiStore((s) => version > 0 && s.artifacts.some((a) => a.path === path && a.version > version))
  const chip = superseded ? { tone: '', key: 'timeline.deliverySuperseded' } : deliveryChip(String(p.status ?? ''))
  const sub = [String(p.kind ?? ''), version > 0 ? `v${version}` : ''].filter(Boolean).join(' · ')
  // 选中态（canvas#artifact，票 16a 补）：该产物正是画布当前页签时 1.5px accent 环，与侧栏成员行「当前打开」同理
  const current = useUiStore((s) => path !== '' && s.canvasOpen && s.activeTab === `art:${path}`)
  return (
    <button type="button" className="tl-deliver" title={path} data-superseded={superseded ? '' : undefined}
      data-current={current ? '' : undefined} aria-current={current ? 'true' : undefined}
      onClick={() => path && openTab({ id: `art:${path}`, kind: 'artifact', title: path, path })}>
      <span className="fi"><Icon name="file-text" size={16} /></span>
      <span className="tl-grow"><span className="p" style={{ display: 'block' }}>{path}</span>{sub && <span className="s">{sub}</span>}</span>
      <span className={`chip ${chip.tone}`}>{t(chip.key)}</span>
      <Icon name="chevron-right" size={14} />
    </button>
  )
}

function OwnerMessage({ item, steered, turnBoundary, turnActive, story }: {
  item: TimelineItem; steered?: Set<number>; turnBoundary?: number; turnActive?: boolean; story?: boolean
}) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  const atoms = useAtomOptions(true)
  const m = item.message!
  const time = fmtTime(item.event.created_at)
  const delivered = steered?.has(m.id)
  const queued = !delivered && turnActive && item.event.id > (turnBoundary ?? -1)
  // C3：时间线内路径块可点，打开文件（委托到气泡，Md 产出的是普通 button）
  const onAtom = (e: MouseEvent<HTMLDivElement>) => {
    const path = (e.target as HTMLElement).closest<HTMLElement>('[data-atom-path]')?.dataset.atomPath
    if (path) openTab({ id: `file:${path}`, kind: 'file', title: path.split('/').pop() || path, path })
  }
  return (
    <MessageContextMenu eventId={item.event.id} body={m.body} className="tl-me" aria-label={t('timeline.owner')}>
      {m.element_refs?.map(reference => <ElementReference key={reference.id} reference={reference} />)}
      {/* 票 03：图片附件引用 chip（D3 只读形，在气泡上方）——字节在 .hexagon/inbox/，渲染引用不渲染图本体 */}
      {m.attachments?.length > 0 && (
        <div className="tl-me-files">
          {/* #32：data-file-path 让消息菜单在 chip 上右键时换成文件菜单（MessageMenu） */}
          {m.attachments.map((a) => <span key={a.path} data-file-path={a.path} className="contents"><CodeChip file>{a.name}</CodeChip></span>)}
        </div>
      )}
      <div className="tl-me-row">
        <CopyMessage text={m.body} />
        <div className="tl-bub" title={time || undefined} onClick={onAtom}>
          <CollapsibleBody text={m.body} unclamped={story} atoms={atoms} />
        </div>
      </div>
      {/* 票 05 steering 状态：steering_injected 事件到 → 已送达；回合进行中未注入 → 下回合生效 */}
      {(delivered || queued) && (
        <div className="tl-me-meta">
          {delivered && <span className="chip ok">{t('timeline.steeringDelivered')}</span>}
          {queued && <span className="chip info">{t('timeline.steeringQueued')}</span>}
        </div>
      )}
    </MessageContextMenu>
  )
}

function AgentMessage({ item, story }: { item: TimelineItem; story?: boolean }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const providers = useUiStore((s) => s.providers)
  const openTab = useUiStore((s) => s.openTab)
  const atoms = useAtomOptions(false)
  const m = item.message!
  const member = team.find((x) => x.id === m.author)
  const title = member?.role ?? m.author
  const time = fmtTime(item.event.created_at)
  return (
    <MessageContextMenu eventId={item.event.id} body={m.body} className="tl-msg">
      {member && (
        <button type="button" className="tl-face" title={t('agent.openTab')} aria-label={`${t('agent.openTab')} · ${member.role}`}
          onClick={() => openTab({ id: `agent:${member.id}`, kind: 'agent', title: member.role, agentId: member.id, role: member.role })}>
          <AgentFace agentId={m.author} role={member.role} />
        </button>
      )}
      <div className="tl-grow">
        <div className="tl-who">
          <b>{title}</b>
          {/* gap B2：头行「名 · 模型 · 时间」（workbench#running）。
              gap B9：模型取这条消息发出时实际用的（MessageRow.model，迁移 0061 起记录）；B2 时 core
              不按消息记模型，换模型后旧消息头会被改写成新模型。旧消息没有记录时回退成员当前模型并标「当前」，
              不冒充当时的模型。
              gap B11：记录的是模型 id，读时按服务商目录映射成展示名（无目录项显示 id），悬停给原 id。 */}
          {(m.model || member?.model || time) && (
            <span className="t">
              {m.model
                ? <span data-msg-model="recorded" title={m.model}>{modelDisplayName(providers, m.model)}</span>
                : member?.model
                  ? <span data-msg-model="current" title={t('agent.modelFallback.hint')}>{t('agent.modelFallback.label', { model: member.model })}</span>
                  : null}
              {(m.model || member?.model) && time ? ' · ' : ''}
              {time}
            </span>
          )}
          <CopyMessage text={m.body} />
        </div>
        <ThinkingRow text={m.thinking} />
        <CollapsibleBody text={m.body} unclamped={story} atoms={atoms} />
      </div>
    </MessageContextMenu>
  )
}

// memo：滚动中贴底翻转/flash 等父级重渲不再连带可见行整棵重渲
//（remark 重解析是最贵的一帧）；store 订阅（team/openTab）不受 memo 挡。
export const EventRow = memo(function EventRow({
  item,
  steered,
  turnBoundary,
  turnActive,
  onJumpEvent,
  story,
}: {
  item: TimelineItem
  steered?: Set<number>
  turnBoundary?: number
  turnActive?: boolean
  onJumpEvent?: (eventId: number) => void
  story?: boolean
}) {
  const { t } = useTranslation()
  const k = item.event.kind
  // 票 08：封闭选择的结果单独成行，不折进系统事件组——负责人要一眼看到派给了谁。
  if (k === 'pm_routed') {
    const held = item.event.payload.held === true
    const rejected = item.event.payload.rejected === true
    const role = String(item.event.payload.role ?? '')
    const instance = item.event.payload.scope === 'role_instances' ? item.event.payload.agent_id : null
    const target = instance ? `${role} · ${String(instance)}` : role
    const label = rejected
      ? t('timeline.routeRejected')
      : held
        ? t('timeline.routeHold')
        : t('timeline.routedTo', { role: target })
    return <Marker data-route={rejected ? 'rejected' : held ? 'hold' : role}>{label}</Marker>
  }
  if (k === 'stage_started') return <StageHeader item={item} />
  if (k === 'return_summary') return <ReturnSummaryMarker item={item} onJumpEvent={onJumpEvent} />
  if (k === 'artifact_delivered') return <DeliveryCard item={item} />
  if (k === 'flag_submitted') {
    const ev = item.event
    const severity = String(ev.payload.severity ?? '')
    return (
      <div className="tl-flag">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          <span className="chip flag"><Icon name="flag" size={12} />{severity}</span>
          <span className="mono">{String(ev.payload.target ?? '')}</span>
          {ev.payload.section ? <span className="tl-note" style={{ margin: 0 }}>· {String(ev.payload.section)}</span> : null}
        </div>
        <div className="tl-note">{String(ev.payload.reason ?? '')}</div>
      </div>
    )
  }
  if (k === 'flag_adjudicated') {
    const ev = item.event
    const agree = Boolean(ev.payload.agree)
    return (
      <Marker>
        <Icon name="flag" size={12} />
        <span className={`chip ${agree ? 'ok' : 'err'}`}>
          {agree ? t('cards.agreeContinue') : t('cards.rejectContinue')}
        </span>
        <span className="mono">{String(ev.payload.flag_id ?? '')}</span>
        {ev.payload.by ? <span className="dim3">· {String(ev.payload.by)}</span> : null}
      </Marker>
    )
  }
  if (isManagedDecision(item)) return <ManagedRow item={item} />
  if (item.message) {
    const isOwner = item.event.kind === 'owner_message' || item.message.author === 'owner'
    return isOwner
      ? <OwnerMessage item={item} steered={steered} turnBoundary={turnBoundary} turnActive={turnActive} story={story} />
      : <AgentMessage item={item} story={story} />
  }
  return <SystemRow item={item} />
})

// ToolChips 移植（beautiful-ui 票 02，借形 MIT slev12397/beautiful-ui）：
// 折叠组展开成逐行 chip——图标+定名+mono 参数片，行点开看 input/result 明细，
// 组尾文件片点开产物 tab。原作的悬停 diff 预览未移植：core 把 fs_write 的
// content scrub 成 bytes（safety.rs），事件里没有增删行数据，不做假预览。
function callSecs(call: ToolCall): string | null {
  if (!call.result) return null
  const ms = parseTime(call.result.event.created_at) - parseTime(call.called.event.created_at)
  if (!Number.isFinite(ms) || ms < 0) return null
  return ms < 1000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms / 1000)}s`
}

/** C7 工具行右侧状态：在途 .ring；落定 ✓ + 耗时；失败 circleX 红「失败」；被拒 neutral「已拒绝」；
 *  结果未知沿用 CallStatus 的 warn 胶囊（unknown 不是成功，可靠性票 08）。 */
function ToolRowStatus({ call }: { call: ToolCall }) {
  const { t } = useTranslation()
  const ok = toolOutcome(call.result)
  if (ok === 'unknown' || ok == null) return <span className="tl-st"><CallStatus ok={ok} /></span>
  const res = call.result?.event.payload as Record<string, unknown> | undefined
  if (ok === false && (res?.state === 'denied' || res?.denied === true)) return <span className="tl-st"><span className="chip">{t('timeline.callDenied')}</span></span>
  const secs = callSecs(call)
  return (
    <span className="tl-st">
      {ok
        ? <span className="chip ok"><Icon name="check" size={12} />{secs}</span>
        : <span className="chip err"><Icon name="circle-x" size={12} />{t('timeline.turnFailed')}</span>}
    </span>
  )
}

function ToolChipRow({ call, delay, animate = true }: { call: ToolCall; delay: number; animate?: boolean }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const p = call.called.event.payload as Record<string, unknown>
  const tool = String(p.tool ?? '')
  const res = call.result?.event.payload as Record<string, unknown> | undefined
  const ok = toolOutcome(call.result)
  const envelope = res?.result as { output?: Record<string, unknown> & { source?: string; status?: string } } | undefined
  const loadedSkill = tool === 'load_skill' && ok === true && envelope?.output?.status === 'loaded' ? envelope.output : undefined
  // host_fs_trash：参数显示原路径（C7 补设计，978bd3e）——toolInputSummary 取 path 即原路径
  const summary = toolInputSummary(p)
  const detail = JSON.stringify({ input: p.input ?? p, ...(res ? { result: res } : {}) }, null, 2)
  return (
    <div style={animate ? { animation: `fade-up 300ms cubic-bezier(0.23,1,0.32,1) ${delay}ms both` } : undefined}>
      <button
        type="button"
        className="tchip-row"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <Icon name={TOOL_ICON[tool] ?? 'tool'} size={14} />
        <span className="tchip-label">
          {t(`agent.${TOOL_LABEL[tool] ?? 'stepTool'}`, { defaultValue: tool })}
        </span>
        {summary && <CodeChip>{summary}</CodeChip>}
        <ToolRowStatus call={call} />
      </button>
      {open && (
        <div className="tchip-detail">
          {(tool.startsWith('computer_') || tool.startsWith('browser_')) && envelope?.output && <DesktopToolDetails key={call.called.event.project_id} projectId={call.called.event.project_id} capturedAt={call.result?.event.created_at} output={envelope.output} />}
          {loadedSkill && <p>{t('agent.skillLoaded')} · {t('agent.skillSource')}: <code>{loadedSkill.source}</code></p>}
          <CodeBlock code={detail} lang="json" />
        </div>
      )}
    </div>
  )
}

export const ToolGroupRow = memo(function ToolGroupRow({ items, callIndex, expanded, idx, onToggle, fresh }: {
  items: TimelineItem[]
  callIndex?: ReadonlyMap<number, ToolCall>
  expanded: boolean
  idx: number
  onToggle: (idx: number) => void
  // fresh=true：本次展开动作后的首挂——入场动画只播这一回；
  // 之后滚动重挂载 fresh=false，不再重放（虚拟列表行会被卸载重建）。
  fresh?: boolean
}) {
  const { t } = useTranslation()
  const openTab = useUiStore((s) => s.openTab)
  // 计数按调用对（called 吸收 result），不按事件条数——旧版 2N 的数会翻倍。
  const calls = useMemo(() => pairToolCalls(items).map((call) => callIndex?.get(call.called.event.id) ?? call), [items, callIndex])
  const files = useMemo(() => {
    const seen = new Set<string>()
    for (const c of calls) {
      const path = toolFilePath(c.called.event.payload as Record<string, unknown>)
      if (path) seen.add(path)
    }
    return [...seen]
  }, [calls])
  if (!calls.length) return null
  // C7：汇总行 ▾ +「N 次工具调用」→ 逐行（读类 chip 行 / 写与执行类 C8 执行卡）→ 文件 chip 行
  return (
    <div className="tl-tools tl-indent">
      <button type="button" className="sysrow tl-sum" aria-expanded={expanded} onClick={() => onToggle(idx)}>
        <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} />
        {t('timeline.toolCalls', { count: calls.length })}
      </button>
      {expanded && (
        <div>
          {/* exec-cards 票 02（spec D2）：重负载升 ExecCard，轻量读系保持 chip 行 */}
          {calls.map((c, i) => {
            const tool = String((c.called.event.payload as Record<string, unknown>).tool ?? '')
            return <div key={c.called.event.id} data-timeline-event={c.called.event.id} data-timeline-result={c.result?.event.id}>
              {EXEC_CARD_TOOLS.has(tool)
                ? <ToolExecCard call={c} delay={Math.min(i, 12) * 45} animate={fresh} />
                : <ToolChipRow call={c} delay={Math.min(i, 12) * 45} animate={fresh} />}
            </div>
          })}
          {files.length > 0 && (
            <div className="tchip-files">
              {files.map((path, i) => (
                <button
                  key={path}
                  type="button"
                  className="chip chip-btn mono"
                  style={fresh ? { animation: `pop-in 250ms cubic-bezier(0.23,1,0.32,1) ${Math.min(i, 10) * 60}ms both` } : undefined}
                  title={path}
                  onClick={() => openTab({ id: `art:${path}`, kind: 'artifact', title: path, path })}
                >
                  {path.split('/').pop() || path}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
})

export const SysGroupRow = memo(function SysGroupRow({ items, expanded, idx, onToggle }: {
  items: TimelineItem[]
  expanded: boolean
  idx: number
  onToggle: (idx: number) => void
}) {
  const { t } = useTranslation()
  return (
    <div>
      <button type="button" className="sysrow tl-sys" aria-expanded={expanded} onClick={() => onToggle(idx)}>
        <span className="syslabel tl-sys-l">
          {t('timeline.sysEvents', { count: items.length })} <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} />
        </span>
      </button>
      {expanded && items.map((it) => <div key={it.event.id} data-timeline-event={it.event.id}><SystemRow item={it} /></div>)}
    </div>
  )
})

// exec-cards 票 03：回合摘要行（Cursor Worked-for-Xs 借形）——已收束回合的
// 执行行收成单行（角色 · 工具数 · 耗时），点开就地展开还原被折行。
// C9：32 高 inset 条 + zap；中性色：摘要是收纳不是信号；failed 回合挂 err「失败」。
const TurnSummaryRow = memo(function TurnSummaryRow({
  row,
  expanded,
  idx,
  onToggle,
  children,
}: {
  row: Extract<ModelRow, { type: 'turnsummary' }>
  expanded: boolean
  idx: number
  onToggle: (idx: number) => void
  children?: ReactNode
}) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  const member = row.agentId ? team.find((x) => x.id === row.agentId) : undefined
  const role = member?.role ?? row.agentId ?? ''
  const secs = row.secs == null ? '?' : row.secs < 1 ? '<1' : String(Math.round(row.secs))
  return (
    <div className="tl-indent">
      <div
        className="sysrow turn-summary"
        role="button"
        tabIndex={0}
        aria-expanded={expanded}
        onClick={() => onToggle(idx)}
        onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onToggle(idx) } }}
      >
        <Icon name="bolt" size={14} />
        <span className="tl-ts-l">{t('timeline.turnSummary', { role, count: row.calls, secs })}</span>
        <span className="tl-sp" />
        {row.failed && <span className="chip err">{t('timeline.turnFailed')}</span>}
        <Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} />
      </div>
      {expanded && <div style={{ marginLeft: -42 }}>{children}{row.partial && <div className="tl-note" style={{ padding: '4px 0 8px 42px' }}>{t('timeline.partialTurn')}</div>}</div>}
    </div>
  )
})

// ---- 流式气泡（票 03 + ui-audit 票 08 交接）：回合进行中的瞬时增量挂在
// 时间线尾部；done 到达不删气泡——原地转落位样式（去掉 streaming 标签，
// 与持久消息行同构同宽），等 refreshFast 拉到同 agent 持久消息才清除。
// 交接语义取「持久化确认后」而非「done 即清」：后者留最长 2s 内容空窗
// + 排版跳变（P1-7 实测），前者气泡与落库行无缝替换。
function joinCalls(buf: Record<number, string> | undefined): string {
  if (!buf) return ''
  return Object.keys(buf)
    .map(Number)
    .sort((a, b) => a - b)
    .map((c) => buf[c])
    .filter((s) => s.length > 0)
    .join('\n\n')
}

// 首 token 等待位（beautiful-ui 票 04）：回合已开、流缓冲未到的死寂段——
// 像素格占住气泡位，告诉负责人 agent 在跑而非卡死。
// 缓冲一到即被 LiveReply 顶掉（ids 判据同一处），无交接空窗。
export function WaitingReply({ role, startedAt, netWaiting, avatar }: {
  role: string
  startedAt?: number | null
  /// 等网进入时刻（票 NR-02）：首 token 未到就断网时，占位气泡的
  /// 标签从「执行中」转「重连中」，计时起点 = 等网起点（不是回合起点）。
  netWaiting?: number
  avatar?: ReactNode
}) {
  const { t } = useTranslation()
  // C9 等待位：头像 + 名 + 像素格 + shimmer「正在思考」+ 等宽计时；等网时标签「重连中」（橙）
  return (
    <div className={`tl-msg tl-wait${netWaiting != null ? ' net' : ''}`} data-waiting="">
      {avatar}
      <div className="tl-grow">
        <div className="tl-who"><b>{role}</b></div>
        <LoadingState
          label={netWaiting != null ? t('timeline.reconnecting') : t('timeline.waitingFirstToken')}
          startedAt={netWaiting ?? startedAt ?? undefined}
        />
      </div>
    </div>
  )
}

function StreamFooter() {
  // 窄订阅：整店订阅会让任何无关 store 更新都重渲 Footer（流式期间每 delta 一次）
  const plans = useUiStore((s) => s.plans)
  const streams = useUiStore((s) => s.streams)
  const thinkings = useUiStore((s) => s.thinkings)
  const streamDone = useUiStore((s) => s.streamDone)
  const waitingSince = useUiStore((s) => s.waitingSince)
  const team = useUiStore((s) => s.team)
  const timeline = useUiStore((s) => s.timeline)
  const timelineCaughtUp = useUiStore((s) => s.timelineCaughtUp)
  const ids = [...new Set([...Object.keys(streams), ...Object.keys(thinkings), ...Object.keys(plans)])].filter((id) =>
    !!plans[id] || Object.values(streams[id] ?? {}).some((s) => s.length > 0) ||
    Object.values(thinkings[id] ?? {}).some((s) => s.length > 0),
  )
  const facts = useUiStore(s => s.timelineFacts)
  const projectRoot = useUiStore(s => s.projectRoot)
  const turn = facts || projectRoot ? factOpenTurn(facts) : openTurn(timeline, timelineCaughtUp)
  const waitingId = turn.agentId && !ids.includes(turn.agentId) ? turn.agentId : null
  const waitingMember = waitingId ? team.find((x) => x.id === waitingId) : undefined
  // 底部留白带恒在内容总高里。贴底写的是 scrollHeight，不是最后一行的底边。
  const tail = <div data-timeline-tail="" style={{ height: TIMELINE_TAIL_PX }} />
  const intake = <IntakeFooter />
  if (!ids.length && !waitingId) return <>{intake}{tail}</>
  return (
    <div>
      {intake}
      <div className="tl-col">
      {waitingId && (
        <WaitingReply
          role={waitingMember?.role ?? waitingId}
          startedAt={turn.at}
          netWaiting={waitingSince[waitingId]}
          avatar={waitingMember ? <AgentFace agentId={waitingId} role={waitingMember.role} /> : undefined}
        />
      )}
      {ids.map((id) => {
        const member = team.find((x) => x.id === id)
        const done = streamDone[id] != null
        return (
          <div key={id}>
          {plans[id] && <div className="plan-row tl-indent"><ThinkingRow plan text={plans[id]}
            live={!done && !Object.keys(streams[id] ?? {}).some((call) => Number(call) > 0)} /></div>}
          <LiveReply
            role={member?.role ?? id}
            model={member?.model}
            text={joinCalls(streams[id])}
            thinking={joinCalls(thinkings[id])}
            generating={!done}
            netWaiting={waitingSince[id]}
            avatar={member ? <AgentFace agentId={id} role={member.role} /> : undefined}
          />
          </div>
        )
      })}
      </div>
      {tail}
    </div>
  )
}

// C13 开场草案卡：首屏时随 FreshStart 居中出现；负责人已开口后仍未处理的草案停在对话尾部。
const FreshContext = createContext(false)
function IntakeFooter() {
  const fresh = useContext(FreshContext)
  if (fresh) return null
  return <div className="tl-col"><IntakeCard /></div>
}

function TimelineTail() { return <div data-timeline-tail="" style={{ height: TIMELINE_TAIL_PX }} /> }

const EMPTY_TIMELINE: TimelineItem[] = []

// ---- 主体 ----

/** 对话列外侧至少留这么宽才显示节点轨（轨宽 36 + 右缘 14，扣掉列内 24 内距后再留一点呼吸）。 */
const RAIL_SPARE_PX = 36

export function Timeline() {
  const { t } = useTranslation()
  const { timeline: legacyTimeline, timelineCaughtUp, pending, streams, team, projectRoot, projectEpoch, timelineFacts } = useUiStore()
  const [filter, setFilter] = useState<Filter>('all')
  const windowState = useTimelineWindow(filter)
  const { page } = windowState
  const { nodes, watermark: nodesWatermark } = useTimelineNodes()
  const timeline = page?.items ?? (projectRoot == null && projectEpoch === 0 ? legacyTimeline : EMPTY_TIMELINE)
  const callIndex = useMemo(() => new Map(pairToolCalls([...timeline, ...(page?.boundary_pairs ?? [])]
    .sort((a, b) => a.event.id - b.event.id)).map(call => [call.called.event.id, call])), [timeline, page?.boundary_pairs])
  const stickReq = useUiStore(s => s.timelineStickReq)
  const [expanded, setExpanded] = useState<Set<number>>(new Set())
  const [flash, setFlash] = useState<string | null>(null)
  const [exported, setExported] = useState<number | null>(null)
  const [jumpTarget, setJumpTarget] = useState<number | null>(null)
  const expandAt = useRef(new Map<number, number>())
  const ANIM_MS = 1000
  const rows = useMemo(() => buildRows(timeline, filter, projectRoot ? page ?? undefined : undefined), [timeline, filter, page, projectRoot])
  const [previousRows, setPreviousRows] = useState({ rows, epoch: projectEpoch })
  if (previousRows.rows !== rows || previousRows.epoch !== projectEpoch) {
    setPreviousRows({ rows, epoch: projectEpoch })
    if (previousRows.epoch !== projectEpoch) setExpanded(new Set())
    else if (windowState.change === 'prepend') setExpanded(expandedAfterPrepend(previousRows.rows, rows, expanded))
  }
  const viewport = useTimelineViewport({ rows, generation: windowState.generation, change: windowState.change,
    loading: windowState.loading || !!windowState.error || (projectEpoch > 0 && projectRoot == null), enabled: projectRoot != null || projectEpoch > 0,
    hasBefore: page?.has_before ?? false, hasAfter: page?.has_after ?? false,
    olderLoading: windowState.olderLoading, olderError: windowState.olderError,
    loadOlder: windowState.loadOlder, loadLatest: windowState.loadLatest,
    targetEventId: page?.target_event_id ?? null, stickReq })
  // 票 06（C15）+ gap A1：宿主节点读给完整历史的四类刻度（水位之后的新事件先从窗口补）；跟随筛选；超 40 道合并事件刻度。
  const marks = useMemo(() => compactMarks(nodeMarks(timeline, rows, pending.length,
    { hostNodes: projectRoot != null ? nodes : undefined, hostWatermark: nodesWatermark, filter })), [projectRoot, nodes, nodesWatermark, rows, pending.length, timeline, filter])
  const [range, setRange] = useState<{ start: number; end: number } | null>(null)
  const highlights = useMemo(() => markHighlights(marks, range), [marks, range])
  // owner-followups-1015 #08：消息右键「在节点轨中定位」只对有刻度的消息可用。
  const railEvents = useMemo(() => new Set(marks.flatMap(m => (m.merged ?? [m]).flatMap(x => x.eventId != null ? [x.eventId] : []))), [marks])
  const areaRef = useRef<HTMLDivElement>(null)
  // 票 16c：虚拟化重挂把聚焦行移出 DOM 时，焦点放回原行（退路：滚动容器），不掉到 body。
  useEffect(() => {
    const area = areaRef.current
    if (!area) return
    return keepTimelineFocus(area, () => area.querySelector<HTMLElement>('[data-virtuoso-scroller]'))
  }, [])
  const jumpBottom = useJumpOffset(areaRef)
  const [compactRail, setCompactRail] = useState(false)
  useLayoutEffect(() => {
    const el = areaRef.current
    if (!el || typeof ResizeObserver === 'undefined') return
    // 窗口过窄或画布挤占：对话列两侧放不下轨道（右缘 14 + 轨宽 36 + 与列的间距）时收起（C15 禁用态）。
    const measure = () => {
      const col = el.querySelector<HTMLElement>('.tl-col')
      const spare = (el.clientWidth - (col?.offsetWidth ?? 0)) / 2
      setCompactRail(el.clientWidth > 0 && spare < RAIL_SPARE_PX)
    }
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    measure()
    return () => observer.disconnect()
  }, [])
  const { steered, turnBoundary } = useMemo(() => {
    const injected = new Set<number>(page?.steered_message_ids ?? [])
    let boundary = timelineFacts?.latest_turn_start_id ?? -1
    for (const it of timeline) {
      if (!timelineFacts && it.event.kind === 'turn_started') boundary = it.event.id
      if (it.event.kind === 'system' && it.event.payload?.kind === 'steering_injected') {
        const mid = Number(it.event.payload.msg_id)
        if (mid) injected.add(mid)
      }
    }
    return { steered: injected, turnBoundary: boundary }
  }, [timeline, page?.steered_message_ids, timelineFacts])
  const turnActive = Object.keys(streams).length > 0
  const watermark = timelineFacts?.latest_event_id ?? timeline.at(-1)?.event.id ?? 0
  const [seenWatermark, setSeenWatermark] = useState(watermark)
  useEffect(() => { if (viewport.pinned) setSeenWatermark(watermark) }, [viewport.pinned, watermark])
  useEffect(() => { setSeenWatermark(watermark) }, [windowState.generation])
  // 票 06（C14）：计数只算消息 / 决策 / 交付（freshTally）；有流式输出时「某某 正在回复」优先。
  const streamDone = useUiStore(s => s.streamDone)
  const replying = Object.keys(streams).filter(agent => !streamDone[agent]).at(-1) ?? null
  const face = useMemo(() => jumpFace(freshTally(timeline, seenWatermark), replying), [timeline, seenWatermark, replying])
  useEffect(() => { viewport.writeTail() }, [streams, viewport.writeTail])
  useEffect(() => {
    if (jumpTarget == null || filter !== 'all') return
    void windowState.jumpAround(jumpTarget)
    setJumpTarget(null)
  }, [filter, jumpTarget, windowState.jumpAround])

  // C7：在途回合的工具组默认展开，已收束回合默认收起（收进 C9 摘要）。expanded 集合记录
  // 「与默认相反」的行：在途组在集合里 = 被负责人手动收起。贴底语义不变——展开只是行高变化，
  // 由 writeTail / onHeight 照旧接管（Timeline.scroll 测试守）。
  const liveGroups = useMemo(() => {
    const open = new Map<string | null, number>()
    for (const it of timeline) {
      const k = it.event.kind
      if (k === 'team_slept' || k === 'stage_started') open.clear()
      else if (k === 'turn_started') open.set(it.event.agent_id, it.event.id)
      else if (k === 'turn_finished' || k === 'turn_failed') open.delete(it.event.agent_id)
    }
    const live = new Set<number>()
    for (const row of rows) {
      if (row.type !== 'toolgroup' || !row.items.length) continue
      const start = open.get(row.items[0].event.agent_id)
      if (start != null && row.items[0].event.id > start) live.add(row.idx)
    }
    return live
  }, [timeline, rows])
  const isOpen = (idx: number) => liveGroups.has(idx) !== expanded.has(idx)

  const revealRow = useCallback((row: ModelRow) => {
    setExpanded(previous => {
      const next = new Set(previous)
      const show = (idx: number) => { if (liveGroups.has(idx)) next.delete(idx); else next.add(idx) }
      if (row.type === 'toolgroup' || row.type === 'sysgroup') show(row.idx)
      if (row.type === 'turnsummary') { next.add(row.idx); for (const child of row.folded) next.add(child.idx) }
      return next
    })
  }, [liveGroups])
  useEffect(() => {
    const target = page?.target_event_id
    if (target == null) return
    const row = rows.find(row => rowContainsEvent(row, target))
    if (row) { revealRow(row); setFlash(rowKey(row)) }
  }, [page?.target_event_id, rows, revealRow])
  useEffect(() => {
    if (flash == null) return
    const timer = setTimeout(() => setFlash(null), 1400)
    return () => clearTimeout(timer)
  }, [flash])
  const jumpToEvent = useCallback((id: number) => {
    const index = rows.findIndex(row => rowContainsEvent(row, id))
    if (index < 0) { viewport.releasePin(); setFilter('all'); setJumpTarget(id); return }
    revealRow(rows[index])
    viewport.jumpToRow(index, id)
    setFlash(rowKey(rows[index]))
  }, [rows, revealRow, viewport.releasePin, viewport.jumpToRow])
  // Managed-followups ticket 05: the delegation report links an external
  // operation to its delegated decision row.
  useEffect(() => {
    const handler = (event: Event) => {
      const id = (event as CustomEvent<unknown>).detail
      if (typeof id === 'number') jumpToEvent(id)
    }
    window.addEventListener(TIMELINE_JUMP_EVENT, handler)
    return () => window.removeEventListener(TIMELINE_JUMP_EVENT, handler)
  }, [jumpToEvent])
  const jump = useCallback((mark: NodeMark) => {
    // 票 06：待决刻度代表「现在」，待决在坞里——点它展开坞，而不是跳到时间线顶。
    if (mark.pending) useUiStore.getState().openPendingDialog()
    else if (mark.eventId != null) jumpToEvent(mark.eventId)
    else if (mark.rowIdx >= 0) jumpToEvent(rows[mark.rowIdx].idx)
  }, [jumpToEvent, rows])
  // 轨迹导出（US54）：kind 过滤随当前筛选档；Tauri 走保存对话框，浏览器 dev 合成 Blob 下载
  const doExport = useCallback(async () => {
    const kinds = filter === 'messages' ? ['owner_message', 'agent_message']
      : filter === 'decisions' ? [...DECISION_KINDS]
      : undefined
    try {
      let count: number
      if (isTauri) {
        const { save } = await import('@tauri-apps/plugin-dialog')
        const path = await save({
          defaultPath: 'hexagon-trace.json',
          filters: [{ name: 'JSON', extensions: ['json'] }],
        })
        if (!path) return
        count = await api.exportEvents({ path, kinds })
      } else {
        const items = await api.exportTimelineItems(kinds)
        const doc = { format: 'hexagon-trace-export', version: 1, count: items.length, events: items.map((i) => i.event) }
        const a = document.createElement('a')
        a.href = URL.createObjectURL(new Blob([JSON.stringify(doc, null, 2)], { type: 'application/json' }))
        a.download = 'hexagon-trace.json'
        a.click()
        URL.revokeObjectURL(a.href)
        count = items.length
      }
      setExported(count)
      setTimeout(() => setExported(null), 2500)
    } catch (e) {
      useUiStore.getState().pushToast(errText(e), 'err')
    }
  }, [filter])
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (useUiStore.getState().modalScope !== 'workbench') return
      if (matches(event, bindingFor('timelineLatest'))) { event.preventDefault(); viewport.holdPin() }
      else if (matches(event, bindingFor('timelineLoadOlder'))) { event.preventDefault(); viewport.requestOlder() }
      else if (matches(event, bindingFor('timelineRetry'))) { event.preventDefault(); void windowState.retry() }
      else if (matches(event, bindingFor('timelineExport'))) { event.preventDefault(); void doExport() }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [viewport.holdPin, viewport.requestOlder, windowState.retry, doExport])
  const toggleRow = useCallback((idx: number) => {
    setExpanded((s) => {
      const n = new Set(s)
      if (n.has(idx)) {
        n.delete(idx)
        expandAt.current.delete(idx)
      } else {
        n.add(idx)
        expandAt.current.set(idx, performance.now())
      }
      return n
    })
  }, [])

  // 「刚展开」窗口内的行播入场动画；窗口外（含滚动重挂载）静止直出。
  const isFreshExpand = (idx: number) =>
    performance.now() - (expandAt.current.get(idx) ?? -1e9) < ANIM_MS

  // exec-cards 票 03：行分发提成可递归闭包——摘要行展开时被折行
  // （toolgroup/sysgroup）走同一套渲染与同一个 expanded 集合（键=行 idx 即事件 id）。
  const rowContent = (row: ModelRow): ReactNode => {
    if (row.type === 'chapter') {
      const role = team.find((m) => m.id === row.agentId)?.role ?? row.agentId ?? ''
      return <Marker>{t('timeline.chapter', { n: row.n, agent: role })}{row.stage ? ` · ${row.stage}` : ''}</Marker>
    }
    if (row.type === 'toolgroup') {
      return <ToolGroupRow items={row.items} callIndex={callIndex} expanded={isOpen(row.idx)} idx={row.idx} onToggle={toggleRow} fresh={isFreshExpand(row.idx)} />
    }
    if (row.type === 'sysgroup') {
      return <SysGroupRow items={row.items} expanded={expanded.has(row.idx)} idx={row.idx} onToggle={toggleRow} />
    }
    if (row.type === 'turnsummary') {
      return (
        <TurnSummaryRow row={row} expanded={expanded.has(row.idx)} idx={row.idx} onToggle={toggleRow}>
          {row.folded.map((r) => <div key={r.idx}>{rowContent(r)}</div>)}
        </TurnSummaryRow>
      )
    }
    return (
      <div className={flash === `item:${row.item.event.id}` ? 'flash-row' : ''}>
        <EventRow item={row.item} steered={steered} turnBoundary={turnBoundary} turnActive={turnActive} onJumpEvent={jumpToEvent} story={filter === 'story'} />
      </div>
    )
  }

  const pinned = useMemo(() => pinnedSummary(timeline), [timeline])
  const delegating = useDelegation().status?.enabled ?? false
  // 完整历史才判首屏：有窗口页就看它没有更早一页；无窗口的旧口径看 legacy 时间线已追平。
  const fullHistory = page != null ? !page.has_before : projectRoot == null && projectEpoch === 0 && timelineCaughtUp && timeline.length > 0
  const fresh = useMemo(() => isFreshProject(timeline, fullHistory), [timeline, fullHistory])
  const Header = useMemo(() => (fresh ? FreshHeader : undefined), [fresh])

  return (
    <TooltipProvider>
    <RailEvents.Provider value={railEvents}>
    <FreshContext.Provider value={fresh}>
    <div className="tl" style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', position: 'relative' }}>
      {/* 过滤（A7 全部 / 消息 / 决策）与导出钮同排，贴在对话列上沿（interaction §1）。
          故事线档（票 18）在模型层保留，界面入口按规范收为三档——见 reconciliation §5 票 05。 */}
      <div className="tl-col tl-bar" style={{ width: '100%', boxSizing: 'border-box' }}>
        <Segmented value={filter === 'story' ? 'all' : filter} onValueChange={(v) => setFilter(v as Filter)} aria-label={t('timeline.filterLabel')}>
          {(['all', 'messages', 'decisions'] as const).map((f) => (
            <SegmentedItem key={f} value={f}>{t(`timeline.filter${f[0].toUpperCase()}${f.slice(1)}`)}</SegmentedItem>
          ))}
        </Segmented>
        <span className="tl-sp" />
        <StatusLine />
        {exported != null && <span className="chip ok" role="status">{t('timeline.exported', { count: exported })}</span>}
        <IconButton size="sm" label={t('timeline.export')} action="timelineExport" onClick={() => void doExport()}>
          <Icon name="export" size={14} />
        </IconButton>
      </div>
      {pinned && (
        <div className="tl-col tl-pin" style={{ width: '100%', boxSizing: 'border-box' }}>
          {/* 票 08（interaction §7 / §8）：托管期间归来，置顶的是托管报告而不是普通归来摘要 */}
          {delegating
            ? <ManagedReport key={pinned.event.id} card />
            : <ReturnSummaryCard key={pinned.event.id} item={pinned} items={timeline} onJumpEvent={jumpToEvent} />}
        </div>
      )}
      <div ref={areaRef} style={{ flex: 1, minHeight: 0, position: 'relative' }}>
        <div aria-busy={windowState.loading || !viewport.ready} style={{ height: '100%', visibility: viewport.ready ? 'visible' : 'hidden' }}>
        <Virtuoso
          key={windowState.generation}
          ref={viewport.ref}
          scrollerRef={viewport.bindScroller}
          firstItemIndex={viewport.firstItemIndex}
          computeItemKey={(_index, row) => rowKey(row)}
          totalListHeightChanged={viewport.onHeight}
          data={rows}
          // overscan：快速滚动时预渲视口上下各 600px，行不再贴边「凭空长出」
          increaseViewportBy={600}
          components={{ Header, Footer: page?.has_after ? TimelineTail : StreamFooter }}
          // 不满一屏顶对齐（不要 alignToBottom）。贴底由 writeTail 写 scrollHeight，
          // followOutput 看不见 Footer 里的留白带，会把最新一行贴回输入框。
          followOutput={false}
          // 票 06（C15 视口内提亮）：可见行范围换算回 rows 下标（firstItemIndex 是前插补偿的偏移）。
          rangeChanged={({ startIndex, endIndex }) => setRange({ start: startIndex - viewport.firstItemIndex, end: endIndex - viewport.firstItemIndex })}
          itemContent={(_i, row) => <div data-timeline-row={rowKey(row)} data-timeline-event={row.type === 'item' || row.type === 'chapter' ? row.idx : undefined} className={`tl-col${flash === rowKey(row) ? ' flash-row' : ''}`}>{rowContent(row)}</div>}
        />
        </div>
        {/* A20：首屏用骨架；读取失败是 C5 行内错误 + 重试，不伪装成空时间线 */}
        {!viewport.ready && <div role={windowState.error ? undefined : 'status'} aria-label={windowState.error ? undefined : t('timeline.loadingRecent')} style={{ position: 'absolute', inset: 0, background: 'var(--app-main)' }}>
          {windowState.error
            ? <div className="tl-col pt-3"><InlineError title={t('timeline.loadFailed')} onRetry={() => void windowState.retry()} retryLabel={t('timeline.retry')} retryTitle={`${t('timeline.retry')} ${formatBinding(bindingFor('timelineRetry'))}`} /></div>
            : <div className="tl-col" aria-busy="true"><SkeletonRows /></div>}
        </div>}
        {/* Owner 2026-10-02: upward intent loads history; only progress or recovery needs a floating surface. */}
        {viewport.ready && (windowState.olderLoading || windowState.olderError || windowState.tailError) &&
          <div style={{ position: 'absolute', top: 0, left: 0, right: 0, zIndex: 14, background: 'var(--app-main)' }}>
            <div className="tl-col">
              {windowState.olderLoading ? <div role="status" aria-busy="true" aria-label={t('timeline.loadingOlder')}><SkeletonRows count={1} /></div>
                : <InlineError variant="row" title={windowState.tailError ? t('timeline.loadFailed') : t('timeline.olderFailedLabel')}
                    onRetry={() => void windowState.retry()} retryLabel={t('timeline.retry')} retryTitle={`${t('timeline.retry')} ${formatBinding(bindingFor('timelineRetry'))}`} />}
            </div>
          </div>}

        {/* 松钉且高过窗口才显，不满一屏钉着、按钮藏着（贴底语义不变：showStickButton / holdPin）。
            票 06（C14）：对话列正中、最上一块底部表面上方 12px；三态见 JumpToLatest。 */}
        {viewport.ready && (page?.has_after || showStickButton(viewport.pinned, viewport.overflow)) && (
          <JumpToLatest face={face} bottom={jumpBottom} onClick={viewport.holdPin} />
        )}
        <JumpAnnouncer face={face} />
        {viewport.ready && <NodeRail marks={marks} highlights={highlights} compact={compactRail} onJump={jump} />}
      </div>
    </div>
    </FreshContext.Provider>
    </RailEvents.Provider>
    </TooltipProvider>
  )
}

function FreshHeader() {
  return <div className="tl-col"><FreshStart /></div>
}
