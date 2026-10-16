import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, formatBinding, matches } from '../keymap'
import { useUiStore } from '../store'
import type { ModelRequestRecovery } from '../gen/ModelRequestRecovery'
import { cn } from '@/lib/utils'
import { Icon } from './Icon'
import { Badge } from './ui/badge'
import { Button } from './ui/button'
import { ShortcutKbd } from './ui/kbd'
import { PixelGrid } from './ui/spinner'
import { Progress } from './ui/progress'

// MR-02 failure codes are produced by core `failure_code`; only these have a
// direct remedy screen. Transient codes (transport, rate_limited,
// provider_unavailable) and invalid_response retry instead.
const SETTINGS_CODES = new Set(['missing_credential', 'credential_store', 'authentication', 'configuration', 'request_rejected'])
const USAGE_CODES = new Set(['budget_unavailable', 'quota'])
// D11 ③「更新钥匙」只给钥匙类失败；配置 / 请求被拒仍叫「打开模型设置」。
const KEY_CODES = new Set(['missing_credential', 'credential_store', 'authentication'])

/** Title/hint keys per item state; `turn` items name the role request. */
function copyKeys(state: string, turn: boolean, intake: boolean): [string, string] {
  if (intake && state !== 'retrying') {
    return state === 'recovered' ? ['modelRecovery.recovered', 'modelRecovery.recoveredHintIntake'] : ['modelRecovery.titleIntake', 'modelRecovery.hintIntake']
  }
  switch (state) {
    case 'recovered': return ['modelRecovery.recovered', turn ? 'modelRecovery.recoveredHintTurn' : 'modelRecovery.recoveredHint']
    case 'retrying': return [turn ? 'modelRecovery.retryingTitleTurn' : 'modelRecovery.retryingTitle', 'modelRecovery.waitingHint']
    case 'stopped': return ['modelRecovery.stopped', 'modelRecovery.stoppedHint']
    case 'needs_choice': return ['modelRecovery.choiceTitle', 'modelRecovery.choiceHint']
    case 'unknown': return ['modelRecovery.unknownTitle', 'modelRecovery.unknownHint']
    case 'superseded': return ['modelRecovery.superseded', '']
    default: return [turn ? 'modelRecovery.titleTurn' : 'modelRecovery.title', turn ? 'modelRecovery.hintTurn' : 'modelRecovery.hint']
  }
}

/** D11 ① 剩余时间 m:ss（向上取整到秒，归零停在 0:00）。 */
export function formatRetryLeft(ms: number): string {
  const s = Math.max(0, Math.ceil(ms / 1000))
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

/** 自动重试窗口内每秒走一格的时钟；不在重试中时不起定时器。 */
function useRetryClock(active: boolean): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (!active) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [active])
  return now
}

// ui-redesign 票 08（components D11 ①–⑨，feedback#model-recovery）：只换皮——状态块
// （① 像素格 + 进度 + 停止 · ②⑧ 橙环卡 + 原因 chip + 次数 / 模型 + 尝试历史 · ③ 钥匙行 ·
// ④ 未完成 chip · ⑤ 两键选择 · ⑥⑦ 收成一行 · ⑨ 托管拒绝行）。按键、动作、宿主调用一律不变；
// 「改用主对话模型重试」仍只有独立键位，批准键只给「重试」；不加「放弃这次派活」（R-09）。
// MR-01: both surfaces describe the same persisted item. Only the original
// failed request can retry; generic stage recovery must never receive it.
export function ModelRecoveryStatus({ id, payload, top = false }: { id: string; payload: Record<string, unknown>; top?: boolean }) {
  const { t } = useTranslation()
  const recovery = (payload.view ?? payload) as ModelRequestRecovery
  const [busy, setBusy] = useState(false)
  const inFlight = useRef(false)
  const state = recovery.state
  const now = useRetryClock(state === 'retrying' && recovery.retry_deadline_ms != null)
  const latestCode = recovery.history?.at(-1)?.code
  const openSettings = (section: 'models' | 'usage') => window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: section }))
  const run = async (action: () => Promise<unknown>) => {
    if (inFlight.current) return
    if (useUiStore.getState().modalScope !== 'workbench') {
      useUiStore.getState().pushToast(t('decisions.scopeBlocked')); return
    }
    inFlight.current = true; setBusy(true)
    try {
      await action()
    } catch (error) {
      useUiStore.getState().pushToast(t('errors.actionFailed', { detail: errText(error) }), 'err')
    } finally {
      try { await useUiStore.getState().invalidate() } finally {
        inFlight.current = false; setBusy(false)
      }
    }
  }
  const label = t('modelRecovery.retry')
  const choosing = state === 'needs_choice'
  const canRetry = recovery.can_retry !== false && !['recovered', 'retrying', 'superseded', 'needs_choice'].includes(state)
  const shortcut = top && canRetry ? formatBinding(bindingFor('approve')) : ''
  const chatOffer = canRetry ? recovery.chat_model_slot : null
  const retryWithChat = () => run(() => api.retryModelRequestWithChatModel(id, recovery.project_root))
  // Ticket 06: stop this item's automatic retries (control lane; the item
  // stays for an explicit retry and nothing else revives it).
  // Not through `run`: the retry it would stop holds the in-flight guard.
  const stop = () => api.stopModelRequest(id)
    .catch(error => useUiStore.getState().pushToast(t('errors.actionFailed', { detail: errText(error) }), 'err'))
  const choose = (followNew: boolean) => run(() => api.resolveModelRequestChoice(id, followNew, recovery.project_root))
  // ADR 0051: the explicit choice has its own bindings on the top item only.
  useEffect(() => {
    if (!top || (!choosing && !chatOffer && state !== 'retrying')) return
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat || e.defaultPrevented) return
      if (state === 'retrying' && matches(e, bindingFor('modelRecoveryStop'))) {
        e.preventDefault(); void stop(); return
      }
      if (chatOffer && matches(e, bindingFor('retryWithChatModel'))) {
        e.preventDefault(); void retryWithChat(); return
      }
      if (!choosing) return
      const followNew = matches(e, bindingFor('followNewInstruction'))
      if (!followNew && !matches(e, bindingFor('continueOriginalTask'))) return
      e.preventDefault()
      void choose(followNew)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })
  const turn = recovery.purpose === 'turn'
  const target = recovery.target_role ?? t('modelRecovery.nextRole')
  const intake = recovery.purpose === 'opening_intake'
  const [title, hint] = copyKeys(state, turn, intake)
  const reason = state === 'superseded' && recovery.superseded_reason ? t(`modelRecovery.reason.${recovery.superseded_reason}`, { defaultValue: recovery.superseded_reason }) : ''
  const refusal = recovery.last_refusal && !['recovered', 'superseded'].includes(state)
    ? t('modelRecovery.lastRefusal', { reason: t(`modelRecovery.refusal.${recovery.last_refusal}`, { defaultValue: recovery.last_refusal }) }) : ''
  // 票 16B（feedback#model-recovery ①、D11）：原写「自动重试截至 <钟点>」，改为规范的「第 N 次 · 剩余 m:ss」倒计时 +
  // 已过时间进度条（窗口长度 retry_window_ms 由 core 随截止时刻一起发；旧事件没有窗口长度时只出倒计时）。
  const leftMs = state === 'retrying' && recovery.retry_deadline_ms != null ? recovery.retry_deadline_ms - now : null
  const windowMs = recovery.retry_window_ms ?? null
  const elapsedPct = leftMs != null && windowMs ? Math.round(100 * (1 - Math.max(0, leftMs) / windowMs)) : null
  const history = recovery.history ?? []
  const latest = history.at(-1)
  const codeLabel = latestCode ? t(`modelRecovery.code.${latestCode}`, { defaultValue: latestCode }) : ''
  // D11 ③：认证 / 配置 / 额度——钥匙行直接给入口。重试按钮保留（宿主 can_retry 仍为真，修好钥匙后
  // 这是唯一的回路；去掉会把事项卡死），只是不再当主按钮——偏差见 reconciliation「票 08」。
  const remedy = canRetry && latestCode && (SETTINGS_CODES.has(latestCode) || USAGE_CODES.has(latestCode)) ? latestCode : null
  const keyRemedy = remedy && KEY_CODES.has(remedy)
  const historyBlock = <details className="mt-2 text-[12px] text-ink-2" data-attempts>
    <summary className="hx-focus cursor-pointer rounded-[6px] live:hover:text-ink">{t('modelRecovery.historyTitle', { count: history.length })}</summary>
    {recovery.instruction && <div className="mt-1 whitespace-pre-wrap">{t('modelRecovery.instruction', { text: recovery.instruction })}</div>}
    {history.map((failure, index) => <div key={index} className="mt-1 font-mono text-[11.5px] leading-[1.7]">
      {failure.model ?? t('modelRecovery.modelSlot', { slot: failure.model_slot })} · {failure.code} · {failure.request_id ?? t('modelRecovery.notSent')}
      {failure.http_status != null && ` · HTTP ${failure.http_status}`} · {failure.message}
    </div>)}
  </details>
  // ⑥ 已恢复 / ⑦ 已失效：块收成一行（原地变化，不新增行）。
  if (state === 'recovered' || state === 'superseded') {
    return <div data-model-recovery={id} data-state={state} aria-busy={busy} className="text-[12.5px]">
      <div className={state === 'recovered' ? 'flex items-center gap-1.5 text-green' : 'text-ink-3'}>
        {state === 'recovered'
          ? <><Icon name="circle-check" size={14} />{t('modelRecovery.recoveredLine', { count: history.length + 1 })}</>
          : <>{t(title)}{reason && ` · ${reason}`}</>}
      </div>
      {state === 'recovered' && hint && <div className="mt-0.5 text-[12px] text-ink-3">{t(hint, { target, instruction: recovery.held_instruction ?? '' })}</div>}
      {state === 'recovered' && history.length > 0 && historyBlock}
    </div>
  }
  const retrying = state === 'retrying'
  const tone = state === 'unknown' ? 'text-ink-3' : 'text-orange'
  return <div data-model-recovery={id} data-state={state} aria-busy={busy}
    className={cn('rounded-[12px] bg-[var(--app-card)] px-3.5 py-3 text-[13px]',
      retrying ? 'shadow-[var(--shadow-card)]' : 'shadow-[0_0_0_1px_color-mix(in_oklch,var(--orange)_35%,transparent),var(--shadow-card)]')}>
    <div className="flex items-center gap-2">
      {retrying ? <PixelGrid /> : <Icon name="warn" size={14} className={cn('flex-none', tone)} />}
      <b className="min-w-0 flex-1 text-[13px] font-semibold text-ink">{t(title)}</b>
      {leftMs != null && <span data-retry-progress className="font-mono text-[12px] text-ink-3">{t('modelRecovery.attempt', { count: history.length + 1, left: formatRetryLeft(leftMs) })}</span>}
      {!retrying && codeLabel && <Badge>{codeLabel}</Badge>}
      {retrying && <Button size="xs" variant="ghost" data-stop-retry
        title={[t('modelRecovery.stopHint'), top ? formatBinding(bindingFor('modelRecoveryStop')) : ''].filter(Boolean).join(' · ')}
        onClick={() => { void stop() }}>{t('modelRecovery.stop')}</Button>}
    </div>
    {hint && <div className={cn('mt-1 text-[12.5px] text-ink-2', retrying && 'ml-[22px]')}>{t(hint, { target, instruction: recovery.held_instruction ?? '' })}</div>}
    {elapsedPct != null && <Progress data-retry-bar tone="accent" value={elapsedPct} aria-label={t('modelRecovery.retryWindow')} className="mt-2" />}
    {refusal && <div data-refusal className="mt-1 text-[12px] text-ink-3">{refusal}</div>}
    {!retrying && history.length > 0 && <div className="mt-1 text-[12px] text-ink-3">
      {t('modelRecovery.retriedCount', { count: history.length })}
      {latest && ` · ${latest.model ?? t('modelRecovery.modelSlot', { slot: latest.model_slot })}`}
    </div>}
    {recovery.unfinished && recovery.partial_text && <details className="mt-2 text-[12px] text-ink-2">
      <summary className="hx-focus flex cursor-pointer items-center gap-1.5 rounded-[6px]"><Badge variant="warn">{t('modelRecovery.unfinishedChip')}</Badge>{t('modelRecovery.unfinished')}</summary>
      <div className="mt-1 text-ink-3">{t('modelRecovery.partialHint')}</div>
      <div className="mt-1 whitespace-pre-wrap font-mono text-[11.5px]">{recovery.partial_text}</div>
    </details>}
    {history.length > 0 && historyBlock}
    {remedy && <div className="mt-2 flex flex-wrap items-center gap-2 rounded-[10px] bg-red-tint px-3 py-2 text-[12.5px] text-red" data-remedy={remedy}>
      <Icon name="key" size={14} className="flex-none" />
      <span className="min-w-0 flex-1"><b className="font-semibold">{codeLabel}</b> · {t('modelRecovery.keyBlocked')}</span>
      {SETTINGS_CODES.has(remedy) && <Button size="xs" title={t(keyRemedy ? 'modelRecovery.updateKey' : 'modelRecovery.openModels')} onClick={() => openSettings('models')}>
        {t(keyRemedy ? 'modelRecovery.updateKey' : 'modelRecovery.openModels')}</Button>}
      {USAGE_CODES.has(remedy) && <Button size="xs" variant="ghost" title={t('modelRecovery.usageQuota')} onClick={() => openSettings('usage')}>{t('modelRecovery.usageQuota')}</Button>}
    </div>}
    {(canRetry || chatOffer || choosing) && <div className="mt-3 flex flex-wrap items-center gap-1.5">
      {canRetry && <Button size="sm" variant={remedy ? 'default' : 'primary'} disabled={busy}
        title={[label, shortcut].filter(Boolean).join(' · ')}
        onClick={() => { void run(() => intake ? api.runOpeningIntake() : api.retryModelRequest(id, recovery.project_root)) }}>
        {busy ? t('modelRecovery.retrying') : label}{shortcut && <ShortcutKbd binding={bindingFor('approve')} />}
      </Button>}
      {chatOffer && <Button size="sm" disabled={busy} data-chat-model
        title={[t('modelRecovery.chatModelHint'), top ? formatBinding(bindingFor('retryWithChatModel')) : ''].filter(Boolean).join(' · ')}
        onClick={() => { void retryWithChat() }}>{t('modelRecovery.useChatModel', { slot: chatOffer })}</Button>}
      {choosing && <>
        <Button size="sm" variant="quiet" disabled={busy} title={[t('modelRecovery.followNew'), formatBinding(bindingFor('followNewInstruction'))].join(' · ')}
          onClick={() => { void choose(true) }}>{t('modelRecovery.followNew')}</Button>
        <Button size="sm" variant="primary" disabled={busy} title={[t('modelRecovery.continueOriginal'), formatBinding(bindingFor('continueOriginalTask'))].join(' · ')}
          onClick={() => { void choose(false) }}>{t('modelRecovery.continueOriginal')}</Button>
      </>}
    </div>}
    {chatOffer && <div className="mt-2 text-[12px] text-ink-3">{t('modelRecovery.chatModelNote')}</div>}
  </div>
}
