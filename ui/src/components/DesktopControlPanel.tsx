// 电脑操作面板（components D19「面板」左块，control#panels；票 ui-redesign 14 换皮）。
// 结构：标题行（monitor + 「电脑操作」+ 状态 chip）→ inset 勾选行（系统授权三行 + 「本项目截图发给模型」项目同意行）
// → 截图准备 / 结果未知提示 → 当前控制者行 → 释放控制 / 清除截图 / 暂停（或恢复）→ 截图数 · 承载授权的应用。
// 系统授权与项目同意是两道独立门，分行显示，一眼看出卡在哪一道（control#panels notes）。
// 放置：默认在头部控制状态的弹层里；`inline` = 设置「权限」分区（此时由本面板挂权限对话框的监听，
// 因为设置页打开时 App 的全局监听随工作台卸下）。语义与换皮前一致：同意、暂停、释放都只走显式按钮或键位。
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { bindingFor, matches, type ActionId } from '../keymap'
import { useUiStore } from '../store'
import { publishDesktopStatus, useDesktopStatus } from '../useDesktopStatus'
import type { DesktopControl } from '../gen/DesktopControl'
import type { DesktopPermissions as Permissions } from '../gen/DesktopPermissions'
import type { DesktopPermission } from '../gen/DesktopPermission'
import { DesktopPermissions } from './DesktopPermissions'
import { ActionButton, CheckRow, insetBlock } from './ComputerParts'
import { Icon } from './Icon'
import { Avatar } from '@/components/ui/avatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { TooltipProvider } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

const controlKeys: Partial<Record<DesktopControl, ActionId>> = {
  enable: 'desktopEnable', disable: 'desktopDisable', pause: 'desktopPause',
  resume: 'desktopResume', release: 'desktopRelease', clear_screenshots: 'desktopClear', prepare_capture: 'desktopPrepareCapture',
}

// 系统授权三行（control#panels）：输入事件与辅助功能在 macOS 同一设置面板里授予。
const GRANT_ROWS = [
  { field: 'accessibility', label: 'control.grantAccessibility', pane: 'accessibility', action: 'desktopAccessibility' },
  { field: 'input_events', label: 'control.grantInput', pane: 'accessibility', action: 'desktopAccessibility' },
  { field: 'screen_recording', label: 'control.grantScreen', pane: 'screen_recording', action: 'desktopScreenRecording' },
] as const satisfies readonly { field: keyof Permissions; label: string; pane: DesktopPermission; action: ActionId }[]

const basename = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) ?? path

export function DesktopControlPanel({ inline = false }: { inline?: boolean }) {
  const { t } = useTranslation()
  const team = useUiStore((s) => s.team)
  // 功能缺口 C4：状态来自共享读（useDesktopStatus），面板不再自开 2 秒定时器；系统授权随每次共享读复查。
  const { status, stamp, error: readError, refresh } = useDesktopStatus()
  const [grants, setGrants] = useState<Permissions | null>(null)
  const [error, setError] = useState('')
  const [pending, setPending] = useState(false)
  const mounted = useRef(true)
  const generation = useRef(0)
  const writing = useRef(false)
  const permissionsWarning = useRef(false)
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false; generation.current++ }
  }, [])
  const enabled = status?.enabled ?? false
  useEffect(() => {
    if (!stamp || writing.current) return
    const ticket = generation.current
    // 票 14：勾选行要显示系统授权，所以停用时也读（只读检测，不弹系统提示）。
    // 缺授权的提醒对话框仍只在已启用时自动弹出（旧语义）。
    api.desktopPermissions().then((permissions) => {
      if (!mounted.current || ticket !== generation.current) return
      setGrants(permissions); setError('')
      if (enabled) {
        if (!permissions.ready && !permissionsWarning.current) window.dispatchEvent(new Event('hexagon:desktop-permissions'))
        permissionsWarning.current = !permissions.ready
      }
    }, (e) => { if (mounted.current && ticket === generation.current) setError(errText(e)) })
  }, [stamp, enabled])
  const control = useCallback(async (action: DesktopControl) => {
    if (writing.current || !status || ((action === 'resume' || action === 'prepare_capture') && status.capture_preparation === 'preparing')) return
    const projectRoot = status.project_root
    const ticket = ++generation.current
    writing.current = true; setPending(true); setError('')
    try {
      if (action === 'enable') {
        const permissions = await api.desktopPermissions()
        // Ticket 07: a delayed preflight from a closed workspace must never
        // enable the new project or open its permission dialog.
        if (!mounted.current || ticket !== generation.current) return
        if (!permissions.ready) { window.dispatchEvent(new Event('hexagon:desktop-permissions')); return }
      }
      const value = await api.desktopControl(action, projectRoot)
      publishDesktopStatus(value)
      window.dispatchEvent(new Event('hexagon:desktop-status-changed'))
      if (mounted.current && action === 'clear_screenshots') window.dispatchEvent(new Event('hexagon:screenshots-cleared'))
    } catch (e) { if (mounted.current) setError(errText(e)) }
    finally { writing.current = false; if (mounted.current) setPending(false) }
  }, [status])
  const openSettings = useCallback(async (permission: DesktopPermission) => {
    // Opening a pane never updates grants. Only the next native check can do so.
    try { await api.desktopOpenSettings(permission) }
    catch (e) { setError(`${t('desktop.settingsFailed')} ${errText(e)}`) }
  }, [t])
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return
      // The application owns the one global pause listener, including Settings.
      // Other changes require the visible panel to avoid accidental authorization.
      for (const [action, id] of Object.entries(controlKeys)) {
        if (id && action !== 'pause' && matches(event, bindingFor(id))) {
          event.preventDefault(); void control(action as DesktopControl); return
        }
      }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [control])

  const action = (name: DesktopControl, disabled = false, variant: 'default' | 'danger' = 'default') => (
    <ActionButton key={name} size="sm" variant={variant} className="flex-1" label={t(`computer.${name}`)} action={controlKeys[name]}
      data-desktop-action={name} disabled={pending || disabled} onClick={() => void control(name)} />
  )
  const shownError = error || readError
  const chip = !status ? null
    : status.paused ? <Badge variant="neutral">{t('control.statusPaused')}</Badge>
    : status.active_agent ? <Badge variant="warn">{t('control.statusBusy')}</Badge>
    : status.enabled ? <Badge variant="ok">{t('control.statusEnabled')}</Badge>
    : <Badge variant="neutral">{t('control.statusDisabled')}</Badge>
  const controller = status?.active_agent
    ? team.find((m) => m.id === status.active_agent)?.role ?? status.active_agent : null
  const elsewhere = status?.active_project != null && status.active_project !== status.project_root
  return (
    <TooltipProvider>
      <section role="region" aria-label={t('computer.title')} aria-busy={pending} data-desktop-panel
        className={cn('flex flex-col text-[13px] text-ink', inline ? '' : 'p-1.5')}>
        <div className="flex items-center gap-2 px-2.5 pt-2.5 pb-2">
          <Icon name="computer" size={14} />
          <b className="flex-1 text-[13px] font-semibold">{t('computer.title')}</b>
          {chip}
        </div>
        {!status && !shownError && <p role="status" className="px-2.5 pb-2 text-[12px] text-ink-3">{t('computer.loading')}</p>}
        <div className={cn(insetBlock, 'mx-1 py-1')} data-desktop-grants>
          {grants && !grants.supported && <CheckRow ok={false} label={t('desktop.unsupported')} />}
          {grants && grants.supported && !grants.available && <CheckRow ok={false} label={t('desktop.unavailable')} />}
          {grants?.supported && grants.available && <>
            {GRANT_ROWS.map(({ field, label, pane, action }) => (
              <CheckRow key={field} ok={grants[field]} label={t(label)}>
                {grants[field] ? <span className="text-ink-3">{t('control.granted')}</span>
                  : <ActionButton size="xs" label={t('control.openSettings')} action={action} onClick={() => void openSettings(pane)} />}
              </CheckRow>
            ))}
          </>}
          {status && <CheckRow ok={status.enabled} label={t('control.consentRow')} data-consent>
            {status.enabled
              ? <><span className="text-ink-3">{t('control.consented')}</span>
                <ActionButton size="xs" variant="ghost" label={t('control.revoke')} action="desktopDisable" data-desktop-action="disable"
                  disabled={pending} onClick={() => void control('disable')} /></>
              : <ActionButton size="xs" label={t('control.agree')} action="desktopEnable" data-desktop-action="enable"
                disabled={pending} onClick={() => void control('enable')} />}
          </CheckRow>}
        </div>
        {status?.enabled && status.capture_preparation && (
          <p role="status" className="px-2.5 pt-2 text-[12px] text-ink-2">{t(`computer.capture_${status.capture_preparation}`)}</p>
        )}
        {status?.enabled && status.capture_preparation !== 'ready' && (
          <div className="px-1.5 pt-1.5">
            {action('prepare_capture', status.busy || status.capture_preparation === 'preparing')}
          </div>
        )}
        {status?.checkpoint && (
          // 功能缺口 C1：离开项目留下的 checkpoint。「继续」= 显式 resume（结清 checkpoint），从不重放被取消的动作。
          <div role="status" data-desktop-checkpoint className="mx-1 mt-2 rounded-[var(--r-card)] bg-orange-tint p-3 text-[12px]">
            <b className="block font-semibold text-ink">{t('control.checkpointTitle')}</b>
            <p className="mt-0.5 text-ink-2">{t('control.checkpointBody', { agent: team.find((m) => m.id === status.checkpoint?.agent)?.role ?? status.checkpoint.agent ?? '—' })}</p>
            {status.checkpoint.interrupted && <p className="mt-0.5 text-ink-2">{t('control.checkpointInterrupted')}</p>}
            <ActionButton size="xs" className="mt-1.5" label={t('control.checkpointContinue')} action="desktopResume" data-checkpoint-continue
              disabled={pending || status.busy} onClick={() => void control('resume')} />
          </div>
        )}
        {status?.outcome_unknown && <p role="status" className="px-2.5 pt-2 text-[12px] text-orange-ink">{t('computer.unknown')}</p>}
        {status && <>
          <div className="flex items-center gap-1.5 px-2.5 pt-2.5 pb-1 text-[12px]" data-controller>
            <span className="flex-1 text-ink-3">{t('control.controller')}</span>
            {controller
              ? <><Avatar size={18} agentId={status.active_agent ?? undefined} name={controller} />
                <span className="truncate">{controller} · {elsewhere ? t('control.otherProject') : basename(status.project_root)}</span></>
              : <span className="text-ink-3">{t('control.noController')}</span>}
          </div>
          <div className="flex gap-1.5 px-1.5 pt-2 pb-1.5">
            {action('release', status.busy || !status.active_agent)}
            {action('clear_screenshots', status.busy || !status.screenshot_count)}
            {status.paused
              ? action('resume', status.busy || status.capture_preparation === 'preparing')
              : action('pause', !status.enabled && !status.active_agent, 'danger')}
          </div>
          <p className="px-2.5 pb-2 text-[12px] text-ink-3">
            {t('computer.screenshots', { count: status.screenshot_count })}
            {grants?.host_name && <> · {t('control.host', { host: grants.host_name })}</>}
          </p>
        </>}
        {shownError && <p role="alert" className="px-2.5 pb-2 text-[12px] text-red-ink">{shownError}</p>}
        {shownError && !status && <div className="px-1.5 pb-1.5"><Button size="xs" onClick={() => void refresh()}>{t('desktop.recheck')}</Button></div>}
        {inline && <DesktopPermissions compact />}
      </section>
    </TooltipProvider>
  )
}
