import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import { bindingFor, matches } from '../keymap'
import { ActionButton } from './ComputerParts'
import { Icon } from './Icon'
import { Button } from '@/components/ui/button'
import { IconButton } from '@/components/ui/icon-button'
import { TooltipProvider } from '@/components/ui/tooltip'
import type { DesktopStatus } from '../gen/DesktopStatus'
import type { ComputerModels } from '../gen/ComputerModels'

// 任务前预检（components D19「预检提示」，control#readiness；票 ui-redesign 14 换皮）：卡 = image 图标块 warn + 标题 + 说明
// + 「检查角色模型」（或截图准备时「电脑操作」）+「仍然发送」+ 关闭 x。只提示不拦截：发送照常可用。
// QA 2026-10-06: discover screenshot/model readiness before a tool refusal.
// Advisory only: text instructions stay usable and bindings stay owner-owned.
/** onSend：「仍然发送」——由输入条传入它自己的发送；不传则不显示该按钮。 */
export function ComputerTaskReadiness({ mentions, onSend }: { mentions: string[]; onSend?: () => void }) {
  const { t } = useTranslation()
  const team = useUiStore(s => s.team)
  const projectEpoch = useUiStore(s => s.projectEpoch)
  const projectRoot = useUiStore(s => s.projectRoot)
  const providers = useUiStore(s => s.providers)
  const [status, setStatus] = useState<DesktopStatus | null>(null)
  const [models, setModels] = useState<ComputerModels | null>(null)
  const [unavailable, setUnavailable] = useState(false)
  const [dismissed, setDismissed] = useState<string | null>(null)
  useEffect(() => {
    let active = true
    let inFlight = false
    setStatus(null); setModels(null); setUnavailable(false)
    if (!projectRoot) return
    const current = () => active && useUiStore.getState().projectEpoch === projectEpoch && useUiStore.getState().projectRoot === projectRoot
    const refresh = async () => {
      if (inFlight) return
      inFlight = true
      try {
        const next = await api.desktopStatus()
        if (!current() || (projectRoot && next.project_root !== projectRoot)) return
        setStatus(next)
        // Fetch metadata when entering/focusing, never authorize/capture here.
        if (next.enabled) {
          try {
            const value = await api.computerModels()
            if (current()) { setModels(value.project_root === next.project_root ? value : null); setUnavailable(value.project_root !== next.project_root) }
          } catch { if (current()) { setModels(null); setUnavailable(true) } }
        } else { setModels(null); setUnavailable(false) }
      } catch { if (current()) { setStatus(null); setModels(null) } }
      finally { inFlight = false }
    }
    void refresh()
    const tick = window.setInterval(() => { if (!document.hidden) void refresh() }, 5000)
    window.addEventListener('focus', refresh)
    window.addEventListener('hexagon:desktop-status-changed', refresh)
    return () => { active = false; window.clearInterval(tick); window.removeEventListener('focus', refresh); window.removeEventListener('hexagon:desktop-status-changed', refresh) }
  }, [team, providers, projectEpoch, projectRoot])
  const openModels = useCallback(() => window.dispatchEvent(new Event('hexagon:computer-model-settings')), [])
  // 设置 › 模型服务（App 的 hexagon:open-settings-section 只收 'models' / 'usage'）
  const openServices = useCallback(() => window.dispatchEvent(new CustomEvent('hexagon:open-settings-section', { detail: 'models' })), [])
  useEffect(() => {
    if (!status?.enabled) return
    const key = (event: KeyboardEvent) => {
      if (!event.defaultPrevented && matches(event, bindingFor('computerModelSettings'))) { event.preventDefault(); openModels() }
      else if (!event.defaultPrevented && matches(event, bindingFor('computerModelServices'))) { event.preventDefault(); openServices() }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [status?.enabled, openModels, openServices])
  const selected = team.filter(role => mentions.length === 0 || mentions.includes(role.role) || mentions.includes(`${role.role}[${role.id}]`))
  const warnings = selected.flatMap(role => {
    const model = models ? models.roles.find(m => m.agent_id === role.id) ?? { agent_id: role.id, vision: 'unknown' as const, model: null } : null
    return model && model.vision !== 'ready' ? [{ role, model }] : []
  })
  const capture = status?.enabled && status.capture_preparation && status.capture_preparation !== 'ready' ? status.capture_preparation : null
  const modelIssue = warnings.length > 0 || unavailable
  // 关闭只收起当前这组提示；提示内容一变（换角色、换模型、准备状态变化）就重新出现。只提示，不拦截。
  const signature = [capture, unavailable, ...warnings.map(({ role, model }) => `${role.id}:${model.vision}`)].join('|')
  if (!status?.enabled || (!capture && !modelIssue) || dismissed === signature) return null
  return <TooltipProvider>
    <aside aria-label={t('computer.title')} data-readiness
      className="rounded-[var(--r-panel)] bg-surface px-4 py-3.5 text-[13px] shadow-[var(--shadow-card)]">
      <div className="flex items-center gap-2.5">
        <span className="grid size-7 flex-none place-items-center rounded-[var(--r-sm)] bg-orange-tint text-orange"><Icon name="image" size={14} /></span>
        <b className="flex-1 text-[13px] font-semibold">{t(modelIssue ? 'control.readinessTitle' : 'control.readinessCapture')}</b>
        <IconButton size="sm" label={t('control.dismiss')} onClick={() => setDismissed(signature)}><Icon name="close" size={12} /></IconButton>
      </div>
      <div className="mt-2 space-y-1 text-ink-2">
        {capture && <p role="status">{t(`computer.capture_${capture}`)}</p>}
        {unavailable && <p role="status">{t('computer.modelsUnavailable')}</p>}
        {warnings.length === 1 && <p role="status">{t(`computer.model_${warnings[0].model.vision}`, { role: warnings[0].role.role })}</p>}
        {warnings.length > 1 && <details><summary className="cursor-pointer">{t('computer.modelsNeedReview', { count: warnings.length })}</summary>
          {warnings.map(({ role, model }) => <p key={role.id}>{t(`computer.model_${model.vision}`, { role: role.role })}</p>)}
        </details>}
        {modelIssue && models && <p className="text-ink-3">{models.vision_slots.length ? t('computer.visionSlots', { slots: models.vision_slots.join(', ') }) : t('computer.noVisionSlots')}</p>}
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        {capture && <ActionButton variant="ghost" size="sm" label={t('computer.title')} action="desktopPanel" data-readiness-computer
          onClick={() => window.dispatchEvent(new Event('hexagon:computer-settings'))}><Icon name="computer" size={12} />{t('computer.title')}</ActionButton>}
        {/* control#readiness 原型「打开模型服务」（票 07 补回）：换成支持视觉的模型先去设置 › 模型服务；
            角色绑的是哪个槽仍在设置 › 团队（「检查角色模型」，票 14 落点保留）。 */}
        {modelIssue && <ActionButton variant="ghost" size="sm" label={t('computer.modelServices')} action="computerModelServices" data-readiness-services
          onClick={openServices}><Icon name="cpu" size={12} />{t('computer.modelServices')}</ActionButton>}
        {modelIssue && <ActionButton variant="ghost" size="sm" label={t('computer.modelSettings')} action="computerModelSettings" data-readiness-models
          onClick={openModels}><Icon name="agent" size={12} />{t('computer.modelSettings')}</ActionButton>}
        <span className="flex-1" />
        {onSend && <Button size="sm" data-readiness-send onClick={() => { setDismissed(signature); onSend() }}>{t('control.sendAnyway')}</Button>}
      </div>
    </aside>
  </TooltipProvider>
}
