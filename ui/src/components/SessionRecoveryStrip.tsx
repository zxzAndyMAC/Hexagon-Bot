// SessionHost 恢复条（components C12a/C12b，补设计）：raised 卡钉在时间线顶部。
// 头行状态 + 当前回合 + 待决档；能力行只表健康（授权已证明才加盾形图标）；
// 工作树行与稍后继续行的动作都只入队负责人确认卡或调用 core，界面不执行 git。
import { useEffect, useState } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { toastError } from '../toast'
import { bindingFor, matches } from '../keymap'
import { useUiStore } from '../store'
import type { CapabilityStatus } from '../gen/CapabilityStatus'
import type { SessionRecord } from '../gen/SessionRecord'
import type { SessionStatus } from '../gen/SessionStatus'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import { CodeChip } from '@/components/ui/code-chip'
import { InlineError } from '@/components/ui/inline-error'
import { Input } from '@/components/ui/input'
import { ShortcutKbd } from '@/components/ui/kbd'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { Icon } from './Icon'
import type { ActionId } from '../keymap'

type BadgeVariant = 'neutral' | 'ok' | 'warn' | 'err' | 'info' | 'line'

const STATUS_VARIANT: Record<SessionStatus, BadgeVariant> = {
  created: 'neutral', running: 'neutral', waiting_user: 'warn', paused: 'info',
  reconnecting: 'warn', completed: 'ok', failed: 'err',
}

const HEALTH_VARIANT: Record<CapabilityStatus['health'], BadgeVariant> = {
  healthy: 'ok', degraded: 'warn', reconnecting: 'warn', unavailable: 'err', unknown: 'line',
}

// 规范 C12a：时间选择四档；fire_at 是 unix 秒。
const LATER_CHOICES: { key: string; at: (now: Date) => number }[] = [
  { key: 'in5', at: (now) => now.getTime() + 5 * 60_000 },
  { key: 'in30', at: (now) => now.getTime() + 30 * 60_000 },
  { key: 'in120', at: (now) => now.getTime() + 120 * 60_000 },
  { key: 'tomorrow9', at: (now) => new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 9).getTime() },
]

function label(t: TFunction, key: string, fallback: string): string {
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

export function SessionRecoveryStrip() {
  const { t } = useTranslation()
  const summary = useUiStore((s) => s.sessionSummary)
  const error = useUiStore((s) => s.sessionError)
  const modalScope = useUiStore((s) => s.modalScope)
  const refreshFast = useUiStore((s) => s.refreshFast)
  const [busy, setBusy] = useState<string | null>(null)
  const [laterOpen, setLaterOpen] = useState(false)
  const [treeOpen, setTreeOpen] = useState(false)
  const [treePath, setTreePath] = useState('')
  const session = summary?.session ?? null
  const reasonCode = session?.last_reason_code ?? summary?.recent_diagnostics[0]?.code ?? null
  const expired = summary?.triggers.filter((trigger) => trigger.state === 'expired') ?? []
  const scheduled = summary?.triggers.find((trigger) => trigger.state === 'scheduled') ?? null
  const intent = session?.worktree_intent ?? null
  const worktreePending = (useUiStore((s) => s.pending)).some((q) => q.kind === 'worktree')
  const terminal = session?.status === 'completed' || session?.status === 'failed'
  // Exhausted budget is terminal for the chain; core refuses reconnect, so do not offer it.
  const exhausted = session?.last_reason_code === 'reconnect_budget_exhausted'

  const act = async (name: string, run: () => Promise<unknown>, done?: string) => {
    if (busy) return
    setBusy(name)
    try {
      await run()
      if (done) useUiStore.getState().pushToast(t(done))
      await refreshFast()
    } catch (e) {
      // 票 13（SessionHost 影响审查 2026-10-13）：恢复条动作失败统一走失败 toast（role=alert），
      // 已登记的原因码（如 worktree_binding_drift，D8）随文案写出。
      toastError(e)
    } finally {
      setBusy(null)
    }
  }
  // A window-close pause has no interrupted run; resume then lifts the host only.
  const resume = () => act('resume', () => api.sessionResume(summary?.recovery_run_id ?? null))
  const schedule = (fireAtMs: number) => {
    setLaterOpen(false)
    void act('later', () => api.sessionScheduleOnce(crypto.randomUUID(), Math.floor(fireAtMs / 1000)))
  }
  const requestCreate = () => {
    setTreeOpen(false)
    void act('tree', () => api.sessionRequestWorktree('create', treePath.trim()), 'session.worktreeRequested')
  }
  const requestCleanup = () => act('tree', () => api.sessionRequestWorktree('cleanup'), 'session.worktreeRequested')
  const verifyTree = () => act('tree', () => api.sessionVerifyWorktree())

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (modalScope !== 'workbench' || event.defaultPrevented || event.repeat || !session) return
      const run = (fn: () => void) => { event.preventDefault(); fn() }
      if (expired[0] && matches(event, bindingFor('sessionContinueTrigger'))) return run(() => void act('continue', () => api.sessionContinueTrigger(expired[0].trigger_id)))
      if (session.status === 'paused' && matches(event, bindingFor('sessionResume'))) return run(() => void resume())
      if (session.status === 'running' && matches(event, bindingFor('sessionPause'))) return run(() => void act('pause', () => api.sessionPause()))
      if (session.status === 'failed' && !exhausted && matches(event, bindingFor('sessionReconnect'))) return run(() => void act('reconnect', () => api.sessionReconnect()))
      if (intent?.state === 'executing' && matches(event, bindingFor('sessionVerifyWorktree'))) return run(() => void verifyTree())
      if (!terminal && !scheduled && matches(event, bindingFor('sessionScheduleOnce'))) return run(() => setLaterOpen(true))
      if (!terminal && !session.worktree && !worktreePending && matches(event, bindingFor('sessionCreateWorktree'))) return run(() => { setTreePath(defaultWorktreePath(session)); setTreeOpen(true) })
      if (session.worktree?.created_by_session && !worktreePending && !intent && matches(event, bindingFor('sessionCleanupWorktree'))) return run(() => void requestCleanup())
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  if (!session && !error) return null

  if (!session) {
    return (
      <Card surface="raised" className="mb-2" aria-live="polite" aria-label={t('session.ariaLabel')} data-session-recovery>
        {/* C12a 错误行 / C5（票 13）：摘要读取失败卡内块形 + 重试；不伪装「无事发生」 */}
        <CardContent>
          <InlineError title={t('session.error')} meta={error} onRetry={() => void refreshFast()} retryLabel={t('session.retry')} />
        </CardContent>
      </Card>
    )
  }

  const tier = summary?.highest_pending_severity
  const worktreeState = intent?.state === 'executing' ? 'wt_unknown'
    : worktreePending ? 'wt_awaiting'
      : intent?.state === 'failed' ? 'wt_failed' : 'wt_attached'
  const worktreeVariant: BadgeVariant = worktreeState === 'wt_attached' ? 'ok' : worktreeState === 'wt_awaiting' ? 'neutral' : 'warn'

  return (
    <Card surface="raised" className="mb-2" aria-live="polite" aria-label={t('session.ariaLabel')} data-session-recovery>
      <CardHeader>
        <CardTitle title={session.session_id}>{t('session.title')}</CardTitle>
        <Badge variant={STATUS_VARIANT[session.status]} loading={session.status === 'reconnecting'}>{t(`session.status_${session.status}`)}</Badge>
        {session.current_turn_id && <span className="text-[12px] text-ink-3">{t('session.currentTurn', { id: session.current_turn_id })}</span>}
        {summary && summary.pending_count > 0 && (
          <Badge variant="warn">{t('session.pendingTier', { count: summary.pending_count, tier: label(t, `session.tier_${tier ?? 'other'}`, 'session.tier_other') })}</Badge>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        {summary && summary.capabilities.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5" data-session-capabilities>
            <span className="text-[12px] text-ink-3">{t('session.capabilities')}</span>
            {summary.capabilities.map((cap) => (
              <Badge key={`${cap.kind}:${cap.id}`} variant={HEALTH_VARIANT[cap.health]} loading={cap.health === 'reconnecting'} title={cap.last_error ?? cap.binding_hint}>
                {label(t, `session.capability_${cap.kind}`, 'session.capability_other')} · {t(`session.health.${cap.health}`)}
                {cap.granted && <span aria-label={t('session.granted')}><Icon name="shield" size={12} /></span>}
              </Badge>
            ))}
          </div>
        )}
        {(session.worktree || intent) && (
          <Card surface="inset" className="flex-row flex-wrap items-center gap-2 px-3 py-2" data-session-worktree>
            <span className="text-[12px] text-ink-3">{t('session.worktree')}</span>
            <CodeChip file>{session.worktree?.path ?? intent?.path ?? ''}</CodeChip>
            <CodeChip>{t('session.baselineCommit', { baseline: session.worktree?.baseline ?? session.baseline, commit: (session.worktree?.baseline_commit ?? intent?.baseline_commit ?? '').slice(0, 8) })}</CodeChip>
            <Badge variant={worktreeVariant} loading={worktreeState === 'wt_awaiting'}>{t(`session.${worktreeState}`)}</Badge>
            {intent?.reason_code && <CodeChip>{intent.reason_code}</CodeChip>}
            <span className="ml-auto flex gap-1.5">
              {intent?.state === 'executing'
                ? <ActionButton action="sessionVerifyWorktree" loading={busy === 'tree'} onClick={() => void verifyTree()}>{t('session.worktreeVerify')}</ActionButton>
                : session.worktree && !worktreePending && (
                  <ActionButton action="sessionCleanupWorktree" loading={busy === 'tree'}
                    disabledReason={session.worktree.created_by_session ? undefined : t('session.worktreeExternal')}
                    onClick={() => session.worktree?.created_by_session && void requestCleanup()}>{t('session.worktreeCleanup')}</ActionButton>
                )}
            </span>
          </Card>
        )}
        {expired.map((trigger) => (
          <div key={trigger.trigger_id} className="flex items-center gap-2" data-session-trigger="expired">
            <Badge variant="warn">{t('session.expired')}</Badge>
            <span className="text-[12px] text-ink-2">{t('session.expiredAt', { time: clock(trigger.fire_at) })}</span>
            <ActionButton action="sessionContinueTrigger" className="ml-auto" loading={busy === 'continue'}
              onClick={() => void act('continue', () => api.sessionContinueTrigger(trigger.trigger_id))}>{t('session.continue')}</ActionButton>
          </div>
        ))}
        {scheduled && <div className="text-[12px] text-ink-2" data-session-trigger="scheduled">{t('session.scheduledAt', { time: clock(scheduled.fire_at) })}</div>}
      </CardContent>
      <CardFooter className="justify-start">
        <span className="text-[12px] text-ink-3">{t('session.nextAction')}</span>
        <span className="text-[13px]">{summary?.next_action ? label(t, `session.action_${summary.next_action}`, 'session.action_pending') : t('session.none')}</span>
        {reasonCode && <CodeChip>{reasonCode}</CodeChip>}
        <span className="ml-auto flex flex-wrap gap-1.5">
          {!terminal && !scheduled && (
            <Popover open={laterOpen} onOpenChange={setLaterOpen}>
              <PopoverTrigger asChild>
                <Button size="sm" variant="ghost">{t('session.scheduleLater')}</Button>
              </PopoverTrigger>
              <PopoverContent aria-label={t('session.scheduleTitle')}>
                <div className="px-2 py-1.5 text-[12px] text-ink-3">{t('session.scheduleTitle')}</div>
                {LATER_CHOICES.map((choice) => (
                  <Button key={choice.key} size="sm" variant="ghost" className="w-full justify-start" onClick={() => schedule(choice.at(new Date()))}>{t(`session.${choice.key}`)}</Button>
                ))}
              </PopoverContent>
            </Popover>
          )}
          {!terminal && !session.worktree && !intent && !worktreePending && (
            <Popover open={treeOpen} onOpenChange={(open) => { if (open) setTreePath(defaultWorktreePath(session)); setTreeOpen(open) }}>
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
          {session.status === 'running' && <ActionButton action="sessionPause" loading={busy === 'pause'} onClick={() => void act('pause', () => api.sessionPause())}>{t('session.pause')}</ActionButton>}
          {session.status === 'failed' && !exhausted && <ActionButton action="sessionReconnect" loading={busy === 'reconnect'} onClick={() => void act('reconnect', () => api.sessionReconnect())}>{t('session.reconnect')}</ActionButton>}
          {session.status === 'paused' && (
            <ActionButton action="sessionResume" loading={busy === 'resume'}
              onClick={() => void resume()}>{t('session.resume')}</ActionButton>
          )}
        </span>
      </CardFooter>
    </Card>
  )
}
