// SessionHost 恢复卡（components C12a/C12b，补设计）。owner-followups-1015 #11（C12a「改为头部胶囊」）起
// 不再钉时间线顶部：本文件只出卡内容（头行 / 能力 / 工作树 / 预约 / 下一步）与动作钩子，
// 由头部「会话」胶囊（SessionPill）在 A16 弹层里渲染；键位也由胶囊宿主挂一份，与胶囊是否出现无关。
// 能力行只表健康（授权已证明才加盾形图标）；工作树行与预约行的动作都只入队负责人确认卡或调用 core，界面不执行 git。
import { useEffect, useState } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { toastError } from '../toast'
import { bindingFor, matches } from '../keymap'
import { useUiStore } from '../store'
import { groupCapabilities, laterBlockedReason, type CapabilityGroup } from '../sessionPill'
import type { CapabilityStatus } from '../gen/CapabilityStatus'
import type { SessionRecord } from '../gen/SessionRecord'
import type { SessionStatus } from '../gen/SessionStatus'
import { Badge, badgeVariants } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { CardContent, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import { Card } from '@/components/ui/card'
import { CodeChip } from '@/components/ui/code-chip'
import { InlineError } from '@/components/ui/inline-error'
import { Input } from '@/components/ui/input'
import { ShortcutKbd } from '@/components/ui/kbd'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { Icon } from './Icon'
import type { ActionId } from '../keymap'

type BadgeVariant = 'neutral' | 'ok' | 'warn' | 'err' | 'info' | 'line'

const STATUS_VARIANT: Record<SessionStatus, BadgeVariant> = {
  created: 'neutral', running: 'neutral', waiting_user: 'warn', paused: 'info',
  reconnecting: 'warn', completed: 'ok', failed: 'err',
}

export const HEALTH_VARIANT: Record<CapabilityStatus['health'], BadgeVariant> = {
  healthy: 'ok', degraded: 'warn', reconnecting: 'warn', unavailable: 'err', unknown: 'line',
}

export function label(t: TFunction, key: string, fallback: string): string {
  return t(key, { defaultValue: t(fallback) })
}

function clock(secs: number): string {
  return new Date(secs * 1000).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' })
}

/** 默认路径：<项目上级目录>/<项目名>-worktrees/<会话短 id>（规范 C12b）。 */
export function defaultWorktreePath(session: Pick<SessionRecord, 'project_root' | 'session_id'>): string {
  const root = session.project_root.replace(/[\\/]+$/, '')
  const cut = Math.max(root.lastIndexOf('/'), root.lastIndexOf('\\'))
  const parent = cut > 0 ? root.slice(0, cut) : root
  const name = root.slice(cut + 1) || 'project'
  const short = session.session_id.replace(/^session-/, '').slice(0, 8)
  return `${parent}/${name}-worktrees/${short}`
}

/** 动作按钮：tooltip 写动作名（禁用时写原因）+ 当前键位（AGENTS UI 约定）。 */
function ActionButton({ action, disabledReason, children, ...props }: React.ComponentProps<typeof Button> & { action: ActionId; disabledReason?: string }) {
  const name = typeof children === 'string' ? children : ''
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button size="sm" aria-disabled={disabledReason ? true : undefined} {...props}>{children}</Button>
      </TooltipTrigger>
      <TooltipContent>{disabledReason ?? name}<ShortcutKbd binding={bindingFor(action)} /></TooltipContent>
    </Tooltip>
  )
}

/** 会话事实 + 动作（卡按钮与键位共用一份 busy，免得键位与按钮并发同一动作）。 */
export function useSessionActions() {
  const { t } = useTranslation()
  const summary = useUiStore((s) => s.sessionSummary)
  const error = useUiStore((s) => s.sessionError)
  const refreshFast = useUiStore((s) => s.refreshFast)
  const pending = useUiStore((s) => s.pending)
  const [busy, setBusy] = useState<string | null>(null)
  const session = summary?.session ?? null
  const expired = summary?.triggers.filter((trigger) => trigger.state === 'expired') ?? []
  const scheduled = summary?.triggers.find((trigger) => trigger.state === 'scheduled') ?? null
  const intent = session?.worktree_intent ?? null
  const worktreePending = pending.some((q) => q.kind === 'worktree')
  const terminal = session?.status === 'completed' || session?.status === 'failed'
  // Exhausted budget is terminal for the chain; core refuses reconnect, so do not offer it.
  const exhausted = session?.last_reason_code === 'reconnect_budget_exhausted'
  // 「稍后继续…」可用：有会话、未结束、尚无预约（规范 C12a；不可用时 ⋯ 菜单 / ⌘K 灰显写原因，判定共用 laterBlockedReason）
  const laterBlocked = laterBlockedReason(summary)

  const act = async (name: string, run: () => Promise<unknown>, done?: string) => {
    if (busy) return
    setBusy(name)
    try {
      await run()
      if (done) useUiStore.getState().pushToast(t(done))
      await refreshFast()
    } catch (e) {
      // 票 13（SessionHost 影响审查 2026-10-13）：恢复卡动作失败统一走失败 toast（role=alert），
      // 已登记的原因码（如 worktree_binding_drift，D8）随文案写出。
      toastError(e)
    } finally {
      setBusy(null)
    }
  }
  return {
    summary, error, session, expired, scheduled, intent, worktreePending, terminal, exhausted, laterBlocked, busy,
    retry: () => void refreshFast(),
    pause: () => act('pause', () => api.sessionPause()),
    reconnect: () => act('reconnect', () => api.sessionReconnect()),
    // A window-close pause has no interrupted run; resume then lifts the host only.
    resume: () => act('resume', () => api.sessionResume(summary?.recovery_run_id ?? null)),
    continueTrigger: (id: string) => act('continue', () => api.sessionContinueTrigger(id)),
    scheduleAt: (fireAtMs: number) => act('later', () => api.sessionScheduleOnce(crypto.randomUUID(), Math.floor(fireAtMs / 1000))),
    cancelBooking: (id: string) => act('cancel', () => api.sessionCancelTrigger(id), 'session.bookingCancelled'),
    requestCreate: (path: string) => act('tree', () => api.sessionRequestWorktree('create', path.trim()), 'session.worktreeRequested'),
    requestCleanup: () => act('tree', () => api.sessionRequestWorktree('cleanup'), 'session.worktreeRequested'),
    verifyTree: () => act('tree', () => api.sessionVerifyWorktree()),
  }
}

export type SessionActions = ReturnType<typeof useSessionActions>

/** 会话键位（interaction §键位表：只在 modalScope = workbench 且该动作此刻可做时生效）。
 * #11 起由胶囊宿主挂一份：胶囊健康时不出现，但键位规则不变——「暂停会话」在运行中照样能按。 */
export function useSessionKeys(a: SessionActions, ui: { openLater: () => void; openTree: () => void }) {
  const modalScope = useUiStore((s) => s.modalScope)
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const { session, expired, scheduled, intent, worktreePending, terminal, exhausted, laterBlocked } = a
      if (modalScope !== 'workbench' || event.defaultPrevented || event.repeat || !session) return
      const run = (fn: () => void) => { event.preventDefault(); fn() }
      if (expired[0] && matches(event, bindingFor('sessionContinueTrigger'))) return run(() => void a.continueTrigger(expired[0].trigger_id))
      if (session.status === 'paused' && matches(event, bindingFor('sessionResume'))) return run(() => void a.resume())
      if (session.status === 'running' && matches(event, bindingFor('sessionPause'))) return run(() => void a.pause())
      if (session.status === 'failed' && !exhausted && matches(event, bindingFor('sessionReconnect'))) return run(() => void a.reconnect())
      if (intent?.state === 'executing' && matches(event, bindingFor('sessionVerifyWorktree'))) return run(() => void a.verifyTree())
      if (!laterBlocked && matches(event, bindingFor('sessionScheduleOnce'))) return run(ui.openLater)
      if (scheduled && matches(event, bindingFor('sessionCancelTrigger'))) return run(() => void a.cancelBooking(scheduled.trigger_id))
      if (!terminal && !session.worktree && !intent && !worktreePending && matches(event, bindingFor('sessionCreateWorktree'))) return run(ui.openTree)
      if (session.worktree?.created_by_session && !worktreePending && !intent && matches(event, bindingFor('sessionCleanupWorktree'))) return run(() => void a.requestCleanup())
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })
}

/** 能力行（C12a #11）：按 kind 合并为一枚「MCP 3/6 正常」，色取最差；点开（aria-expanded）逐项列 id + health，
 * 悬停 title 也列。曾经每项一枚同名胶囊并排（「MCP · 正常」「MCP · 不可用」…），分不清是哪个服务。 */
function CapabilityGroups({ groups }: { groups: CapabilityGroup[] }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState<string | null>(null)
  const kindName = (kind: string) => label(t, `session.capability_${kind}`, 'session.capability_other')
  const shown = groups.find((g) => g.kind === open)
  return (
    <div className="flex flex-col gap-1.5" data-session-capabilities>
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="text-[12px] text-ink-3">{t('session.capabilities')}</span>
        {groups.map((g) => {
          const kind = kindName(g.kind)
          const expanded = open === g.kind
          return (
            <button key={g.kind} type="button" data-session-capability-group={g.kind} data-variant={HEALTH_VARIANT[g.worst]}
              aria-expanded={expanded} aria-label={`${t('session.capGroup', { kind, healthy: g.healthy, total: g.total })} · ${t('session.capGroupItems', { kind })}`}
              title={g.items.map((c) => `${c.id} · ${t(`session.health.${c.health}`)}`).join('\n')}
              className={cn(badgeVariants({ variant: HEALTH_VARIANT[g.worst] }), 'hx-press hx-focus cursor-pointer')}
              onClick={() => setOpen(expanded ? null : g.kind)}>
              {t('session.capGroup', { kind, healthy: g.healthy, total: g.total })}
              <Icon name={expanded ? 'chevron-up' : 'chevron-down'} size={12} />
            </button>
          )
        })}
      </div>
      {shown && (
        <ul className="flex flex-wrap gap-1.5" data-session-capability-items={shown.kind}>
          {shown.items.map((cap) => (
            <li key={`${cap.kind}:${cap.id}`}>
              <Badge variant={HEALTH_VARIANT[cap.health]} loading={cap.health === 'reconnecting'} title={cap.last_error ?? cap.binding_hint}>
                {cap.id} · {t(`session.health.${cap.health}`)}
                {cap.granted && <span aria-label={t('session.granted')}><Icon name="shield" size={12} /></span>}
              </Badge>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

/** 卡内容（弹层里渲染，外壳由 A16 弹层承担，不再叠一层 raised 卡）。 */
export function SessionRecoveryCard({ actions: a, treeOpen, onTreeOpen }: {
  actions: SessionActions; treeOpen: boolean; onTreeOpen: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const [treePath, setTreePath] = useState('')
  const { summary, session, error, expired, scheduled, intent, worktreePending, terminal, exhausted, busy } = a
  useEffect(() => { if (treeOpen && session) setTreePath(defaultWorktreePath(session)) }, [treeOpen, session])

  if (!session) {
    return (
      <div aria-label={t('session.ariaLabel')} data-session-recovery>
        {/* C12a 错误行 / C5（票 13）：摘要读取失败块形 + 重试；不伪装「无事发生」 */}
        <CardContent>
          <InlineError title={t('session.error')} meta={error} onRetry={a.retry} retryLabel={t('session.retry')} />
        </CardContent>
      </div>
    )
  }

  const reasonCode = session.last_reason_code ?? summary?.recent_diagnostics[0]?.code ?? null
  const tier = summary?.highest_pending_severity
  const worktreeState = intent?.state === 'executing' ? 'wt_unknown'
    : worktreePending ? 'wt_awaiting'
      : intent?.state === 'failed' ? 'wt_failed' : 'wt_attached'
  const worktreeVariant: BadgeVariant = worktreeState === 'wt_attached' ? 'ok' : worktreeState === 'wt_awaiting' ? 'neutral' : 'warn'
  const requestCreate = () => { onTreeOpen(false); void a.requestCreate(treePath) }

  return (
    <div aria-label={t('session.ariaLabel')} data-session-recovery>
      <CardHeader>
        <CardTitle title={session.session_id}>{t('session.title')}</CardTitle>
        <Badge variant={STATUS_VARIANT[session.status]} loading={session.status === 'reconnecting'}>{t(`session.status_${session.status}`)}</Badge>
        {session.current_turn_id && <span className="text-[12px] text-ink-3">{t('session.currentTurn', { id: session.current_turn_id })}</span>}
        {summary && summary.pending_count > 0 && (
          <Badge variant="warn">{t('session.pendingTier', { count: summary.pending_count, tier: label(t, `session.tier_${tier ?? 'other'}`, 'session.tier_other') })}</Badge>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {summary && summary.capabilities.length > 0 && <CapabilityGroups groups={groupCapabilities(summary.capabilities)} />}
        {(session.worktree || intent) && (
          <Card surface="inset" className="flex-row flex-wrap items-center gap-2 px-3 py-2" data-session-worktree>
            <span className="text-[12px] text-ink-3">{t('session.worktree')}</span>
            <CodeChip file>{session.worktree?.path ?? intent?.path ?? ''}</CodeChip>
            <CodeChip>{t('session.baselineCommit', { baseline: session.worktree?.baseline ?? session.baseline, commit: (session.worktree?.baseline_commit ?? intent?.baseline_commit ?? '').slice(0, 8) })}</CodeChip>
            <Badge variant={worktreeVariant} loading={worktreeState === 'wt_awaiting'}>{t(`session.${worktreeState}`)}</Badge>
            {intent?.reason_code && <CodeChip>{intent.reason_code}</CodeChip>}
            <span className="ml-auto flex gap-1.5">
              {intent?.state === 'executing'
                ? <ActionButton action="sessionVerifyWorktree" loading={busy === 'tree'} onClick={() => void a.verifyTree()}>{t('session.worktreeVerify')}</ActionButton>
                : session.worktree && !worktreePending && (
                  <ActionButton action="sessionCleanupWorktree" loading={busy === 'tree'}
                    disabledReason={session.worktree.created_by_session ? undefined : t('session.worktreeExternal')}
                    onClick={() => session.worktree?.created_by_session && void a.requestCleanup()}>{t('session.worktreeCleanup')}</ActionButton>
                )}
            </span>
          </Card>
        )}
        {expired.map((trigger) => (
          <div key={trigger.trigger_id} className="flex items-center gap-2" data-session-trigger="expired">
            <Badge variant="warn">{t('session.expired')}</Badge>
            <span className="text-[12px] text-ink-2">{t('session.expiredAt', { time: clock(trigger.fire_at) })}</span>
            <ActionButton action="sessionContinueTrigger" className="ml-auto" loading={busy === 'continue'}
              onClick={() => void a.continueTrigger(trigger.trigger_id)}>{t('session.continue')}</ActionButton>
          </div>
        ))}
        {scheduled && (
          <div className="flex items-center gap-2" data-session-trigger="scheduled">
            <span className="text-[12px] text-ink-2">{t('session.scheduledAt', { time: clock(scheduled.fire_at) })}</span>
            <ActionButton action="sessionCancelTrigger" variant="ghost" className="ml-auto" loading={busy === 'cancel'}
              onClick={() => void a.cancelBooking(scheduled.trigger_id)}>{t('session.cancelBooking')}</ActionButton>
          </div>
        )}
      </CardContent>
      {/* 票 16c（fr 冒烟）：下一步行在动作放不下时整组换到下一行，而不是把「Rien à faire」挤成两行、原因码挤成省略号。
          #11：「稍后继续…」移到头部 ⋯ 菜单与 ⌘K（规范 C12a），卡里只留已预约 / 已过期行。 */}
      <CardFooter className="flex-wrap justify-start" data-session-footer>
        <span className="text-[12px] whitespace-nowrap text-ink-3">{t('session.nextAction')}</span>
        <span className="text-[13px] whitespace-nowrap">{summary?.next_action ? label(t, `session.action_${summary.next_action}`, 'session.action_pending') : t('session.none')}</span>
        {reasonCode && <CodeChip className="shrink-0">{reasonCode}</CodeChip>}
        <span className="ml-auto flex flex-wrap justify-end gap-1.5">
          {!terminal && !session.worktree && !intent && !worktreePending && (
            <Popover open={treeOpen} onOpenChange={onTreeOpen}>
              <PopoverTrigger asChild>
                <Button size="sm" variant="ghost">{t('session.worktreeCreate')}</Button>
              </PopoverTrigger>
              <PopoverContent className="w-[360px] p-3" aria-label={t('session.worktreeCreateTitle')}>
                <form className="flex flex-col gap-2" onSubmit={(e) => { e.preventDefault(); requestCreate() }}>
                  <div className="text-[12.5px] font-medium">{t('session.worktreeCreateTitle')}</div>
                  <Input autoFocus aria-label={t('session.worktreePath')} value={treePath} onChange={(e) => setTreePath(e.target.value)} className="font-mono text-[12px]" />
                  <div className="text-[12px] text-ink-3">{t('session.worktreePath')}</div>
                  <Button size="sm" variant="primary" type="submit" className="self-end" aria-disabled={!treePath.trim() || undefined}>{t('session.worktreeRequest')}</Button>
                </form>
              </PopoverContent>
            </Popover>
          )}
          {session.status === 'running' && <ActionButton action="sessionPause" loading={busy === 'pause'} onClick={() => void a.pause()}>{t('session.pause')}</ActionButton>}
          {session.status === 'failed' && !exhausted && <ActionButton action="sessionReconnect" loading={busy === 'reconnect'} onClick={() => void a.reconnect()}>{t('session.reconnect')}</ActionButton>}
          {session.status === 'paused' && (
            <ActionButton action="sessionResume" loading={busy === 'resume'} onClick={() => void a.resume()}>{t('session.resume')}</ActionButton>
          )}
        </span>
      </CardFooter>
    </div>
  )
}
