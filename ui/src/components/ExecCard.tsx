// exec-cards 票 02：统一执行卡 chrome（Devin 借形）——重负载执行单元
// （bash/fs_patch/fs_write/artifact_write）共用一壳：头行 [icon] 定名
// [meta chips] [copy][chevron]，体按工具分体。轻量读系仍走 ToolChipRow——
// 分层密度是 spec D2 裁决：chip 管扫读、卡管细看。
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { useUiStore } from '../store'
import { diffLines, type DiffOp } from '../diff'
import { Icon, type IconName } from './Icon'
import { CodeBlock } from './Md'
import { DiffView } from './DiffView'
import { SpinnerRing } from './SpinnerRing'
import { toolInputSummary, toolOutcome, TOOL_ICON, type ToolCall, type ToolOutcome } from '../agentSteps'

// 渲染上限（票 04）：实时流/结果体只渲末 32KB——长输出（cargo test 全量）
// 不炸 DOM；缓冲层另有 128KB 尾留（store.toolStreams）。
const TAIL_CAP = 32 * 1024

function tail(s: string): { text: string; trimmed: boolean } {
  return s.length > TAIL_CAP ? { text: s.slice(-TAIL_CAP), trimmed: true } : { text: s, trimmed: false }
}

// tool_result 载荷双层：{tool, ok, result:{output|error}}（tools/mod.rs）。
// 工具返回值 v 在 result.output；错误在 result.error。
function resultOutput(call: ToolCall): Record<string, unknown> | undefined {
  const res = call.result?.event.payload as Record<string, unknown> | undefined
  const inner = res?.result as Record<string, unknown> | undefined
  return inner?.output as Record<string, unknown> | undefined
}

// 票 05 TaskRows 状态机，与 ToolChipRow/AgentTab 同一套语义：
// 无 result=在途运行环；落定翻 check/X 徽标 pop-in。
export function CallStatus({ ok }: { ok: ToolOutcome }) {
  const { t } = useTranslation()
  if (ok === 'unknown') return <span className="chip warn"><Icon name="help" size={8} />{t('cards.actionUnknown')}</span>
  if (ok == null) return <SpinnerRing />
  return (
    <span
      className={`chip ${ok ? 'ok' : 'err'}`}
      style={{ animation: 'pop-in 250ms cubic-bezier(0.23,1,0.32,1) both' }}
    >
      <Icon name={ok ? 'check' : 'close'} size={8} />{ok ? 'ok' : 'err'}
    </span>
  )
}

function CopyBtn({ text }: { text: string }) {
  const { t } = useTranslation()
  const pushToast = useUiStore((s) => s.pushToast)
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout>>(null)
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current) }, [])
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(() => setCopied(false), 1500)
    } catch (e) {
      pushToast(errText(e), 'err')
    }
  }
  return (
    <button
      type="button"
      className="icon-btn exec-copy"
      title={copied ? t('md.copied') : t('md.copy')}
      onClick={copy}
    >
      <Icon name={copied ? 'check' : 'copy'} size={11} />
    </button>
  )
}

/** 执行卡壳：收态=一行（与 chip 行同密度），展开出体。 */
export function ExecCard({
  icon,
  title,
  meta,
  ok,
  copyText,
  delay = 0,
  animate = true,
  children,
}: {
  icon: IconName
  title: string
  meta?: ReactNode
  ok: ToolOutcome
  copyText?: string
  delay?: number
  // 虚拟列表里滚回视口的行会重挂载——入场动画只该在「刚展开」播一次，
  // 重挂载再播就是闪烁（owner 反馈：快速滚动抖动）。Timeline 传 fresh。
  animate?: boolean
  children?: ReactNode
}) {
  const [open, setOpen] = useState(false)
  const toggle = () => setOpen((v) => !v)
  return (
    <div className="exec-card" style={animate ? { animation: `fade-up 300ms cubic-bezier(0.23,1,0.32,1) ${delay}ms both` } : undefined}>
      <div className="exec-head">
        <button type="button" className="exec-toggle" aria-expanded={open} onClick={toggle}>
          <span className="dim3" style={{ display: 'inline-flex' }}><Icon name={icon} size={10} /></span>
          <span className="exec-title mono" title={title}>{title}</span>
          {meta}
          <span style={{ flex: 1 }} />
          <CallStatus ok={ok} />
        </button>
        {copyText != null && <CopyBtn text={copyText} />}
        <button type="button" className="exec-chev" tabIndex={-1} aria-hidden onClick={toggle}>
          <Icon name={open ? 'chevron-down' : 'chevron-right'} size={9} />
        </button>
      </div>
      {open && <div className="exec-body">{children}</div>}
    </div>
  )
}

// ---- 体 ----

function BashBody({ call }: { call: ToolCall }) {
  const { t } = useTranslation()
  const p = call.called.event.payload as Record<string, unknown>
  const inp = (p.input ?? {}) as Record<string, unknown>
  const cmd = String(inp.cmd ?? '')
  const agentId = call.called.event.agent_id ?? ''
  const seq = p.seq != null ? String(p.seq) : '·'
  // 票 04 通道：在途渲实时缓冲；落定定格 result.output（持久层为准）。
  const live = useUiStore((s) => s.toolStreams[`${agentId}:${seq}`])
  const out = resultOutput(call)
  const settled = call.result != null
  const text = settled
    ? [out?.stdout, out?.stderr].filter((x): x is string => typeof x === 'string' && x.length > 0).join('\n')
    : (live ?? '')
  const { text: shown, trimmed } = tail(text)
  return (
    <>
      {cmd && <div className="exec-cmd mono">{cmd}</div>}
      {shown
        ? <pre className="exec-out mono">{trimmed ? `${t('exec.tail')}\n` : ''}{shown}</pre>
        : settled && <div className="dim3" style={{ padding: '8px 10px', fontSize: 11 }}>{t('exec.noOutput')}</div>}
    </>
  )
}

function PatchBody({ call }: { call: ToolCall }) {
  const p = call.called.event.payload as Record<string, unknown>
  const inp = (p.input ?? {}) as Record<string, unknown>
  // 票 01：scrub 放行 old/new 各 ≤4KB——超界段尾带 …[truncated] marker，
  // 残本原样渲染（标明了不是全文）。
  const oldT = typeof inp.old === 'string' ? inp.old : ''
  const newT = typeof inp.new === 'string' ? inp.new : ''
  const ops = useMemo(() => diffLines(oldT.split('\n'), newT.split('\n')), [oldT, newT])
  return <DiffView ops={ops} />
}

function ArtifactBody({ call, onOps }: { call: ToolCall; onOps: (ops: DiffOp[] | null) => void }) {
  const p = call.called.event.payload as Record<string, unknown>
  const inp = (p.input ?? {}) as Record<string, unknown>
  const path = String(inp.path ?? p.path ?? '')
  // 版本按 artifact_id 精确查产物表——同 path 最新版本会把后来
  // 的写入误挂到这张旧卡上。v1 无前任，diffLines([], cur) 全 +。
  const artId = String(resultOutput(call)?.artifact_id ?? '')
  const version = useUiStore((s) =>
    s.artifacts.find((a) => a.id === artId)?.version ?? 0)
  const [ops, setOps] = useState<DiffOp[] | null>(null)
  useEffect(() => {
    // 版本 diff（spec：artifact_write 白拿——版本链本来就在）。
    // 非 Tauri 的浏览器 dev 走 api mock 层，同样能拿到产物内容。
    if (!path || !version) return
    let live = true
    void Promise.all([
      api.artifactContentAt(path, version),
      version > 1 ? api.artifactContentAt(path, version - 1) : Promise.resolve(null),
    ]).then(([cur, prev]) => {
      if (!live || cur == null) return
      const o = diffLines((prev ?? '').split('\n'), cur.split('\n'))
      setOps(o)
      onOps(o)
    }).catch(() => {})
    return () => { live = false }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, version])
  if (!version || !ops) return null
  return <DiffView ops={ops} />
}

function JsonBody({ call }: { call: ToolCall }) {
  const p = call.called.event.payload as Record<string, unknown>
  const res = call.result?.event.payload as Record<string, unknown> | undefined
  const detail = JSON.stringify({ input: p.input ?? p, ...(res ? { result: res } : {}) }, null, 2)
  return <CodeBlock code={detail} lang="json" />
}

// ---- 分发 ----

const fmtBytes = (n: number) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`)

/** ToolGroupRow/AgentTab 共用的重负载卡分发。轻量工具走 ToolChipRow 不进这里。 */
export function ToolExecCard({ call, delay = 0, animate = true }: { call: ToolCall; delay?: number; animate?: boolean }) {
  const { t } = useTranslation()
  const p = call.called.event.payload as Record<string, unknown>
  const tool = String(p.tool ?? '')
  const ok = toolOutcome(call.result)
  const inp = (p.input ?? {}) as Record<string, unknown>
  const out = resultOutput(call)
  const [artOps, setArtOps] = useState<DiffOp[] | null>(null)
  // artifact_write 的 v{n} 徽标：按返回值的 artifact_id 查产物表取版本——
  // 比同 path 最新版本准（后来的写入不会误挂到这张旧卡）。
  const artId = String(out?.artifact_id ?? '')
  const artVersion = useUiStore((s) =>
    tool === 'artifact_write' ? s.artifacts.find((a) => a.id === artId)?.version ?? 0 : 0)

  if (tool === 'bash') {
    const cmd = String(inp.cmd ?? '')
    const first = cmd.split('\n')[0]
    const title = first.length > 80 ? `${first.slice(0, 77)}…` : first
    const code = out?.exit_code
    return (
      <ExecCard
        icon={TOOL_ICON[tool] ?? 'tool'}
        title={title || tool}
        ok={ok}
        copyText={cmd || undefined}
        delay={delay}
        animate={animate}
        meta={typeof code === 'number' && code !== 0
          ? <span className="chip err">{t('exec.exit', { code })}</span>
          : undefined}
      >
        <BashBody call={call} />
      </ExecCard>
    )
  }

  const path = String(inp.path ?? p.path ?? '')
  if (tool === 'fs_patch') {
    const added = out?.diff_added
    const removed = out?.diff_removed
    // spec D6：无字段不挂徽标——缺数据 ≠ 零变化。徽标只写真 diff 来源。
    const stats = (
      <>
        {typeof added === 'number' && <span className="exec-stat ins">+{added}</span>}
        {typeof removed === 'number' && <span className="exec-stat del">−{removed}</span>}
      </>
    )
    return (
      <ExecCard icon={TOOL_ICON[tool] ?? 'tool'} title={path || tool} ok={ok} delay={delay} animate={animate}
        copyText={typeof inp.new === 'string' ? inp.new : undefined} meta={stats}>
        <PatchBody call={call} />
      </ExecCard>
    )
  }

  if (tool === 'artifact_write') {
    const stats = artOps && (
      <>
        <span className="exec-stat ins">+{artOps.filter((o) => o.type === 'ins').length}</span>
        <span className="exec-stat del">−{artOps.filter((o) => o.type === 'del').length}</span>
      </>
    )
    return (
      <ExecCard icon={TOOL_ICON[tool] ?? 'tool'} title={path || tool} ok={ok} delay={delay} animate={animate}
        copyText={path || undefined}
        meta={<>{artVersion > 0 && <span className="chip mono">v{artVersion}</span>}{stats}</>}>
        <ArtifactBody call={call} onOps={setArtOps} />
      </ExecCard>
    )
  }

  // fs_write 与其余重负载兜底：bytes 徽标 + JSON 明细（D4 不装伪 diff）。
  const bytes = typeof inp.bytes === 'number' ? inp.bytes : null
  return (
    <ExecCard icon={TOOL_ICON[tool] ?? 'tool'} title={path || toolInputSummary(p) || tool} ok={ok} delay={delay} animate={animate}
      copyText={path || undefined}
      meta={bytes != null ? <span className="chip mono">{fmtBytes(bytes)}</span> : undefined}>
      <JsonBody call={call} />
    </ExecCard>
  )
}

/** 体单独复用位（AgentTab 步骤明细）：步骤行本身已作头，这里只出借体。 */
export function ExecBody({ call }: { call: ToolCall }) {
  const tool = String((call.called.event.payload as Record<string, unknown>).tool ?? '')
  if (tool === 'bash') return <BashBody call={call} />
  if (tool === 'fs_patch') return <PatchBody call={call} />
  if (tool === 'artifact_write') return <ArtifactBody call={call} onOps={() => {}} />
  return <JsonBody call={call} />
}
