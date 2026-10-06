import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '../api'
import { useUiStore } from '../store'
import { bindingFor, formatBinding, matches } from '../keymap'
import type { DesktopStatus } from '../gen/DesktopStatus'
import type { ComputerModels } from '../gen/ComputerModels'

// QA 2026-10-06: discover screenshot/model readiness before a tool refusal.
// Advisory only: text instructions stay usable and bindings stay owner-owned.
export function ComputerTaskReadiness({ mentions }: { mentions: string[] }) {
  const { t } = useTranslation()
  const team = useUiStore(s => s.team)
  const projectEpoch = useUiStore(s => s.projectEpoch)
  const projectRoot = useUiStore(s => s.projectRoot)
  const providers = useUiStore(s => s.providers)
  const [status, setStatus] = useState<DesktopStatus | null>(null)
  const [models, setModels] = useState<ComputerModels | null>(null)
  const [unavailable, setUnavailable] = useState(false)
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
  useEffect(() => {
    if (!status?.enabled) return
    const key = (event: KeyboardEvent) => {
      if (!event.defaultPrevented && matches(event, bindingFor('computerModelSettings'))) { event.preventDefault(); openModels() }
    }
    window.addEventListener('keydown', key)
    return () => window.removeEventListener('keydown', key)
  }, [status?.enabled, openModels])
  if (!status?.enabled) return null
  const selected = team.filter(role => mentions.length === 0 || mentions.includes(role.role) || mentions.includes(`${role.role}[${role.id}]`))
  const warnings = selected.flatMap(role => {
    const model = models ? models.roles.find(m => m.agent_id === role.id) ?? { agent_id: role.id, vision: 'unknown' as const, model: null } : null
    return model && model.vision !== 'ready' ? [{ role, model }] : []
  })
  return <aside aria-label={t('computer.title')}>
    {status.capture_preparation && status.capture_preparation !== 'ready' && <>
      <p role="status">{t(`computer.capture_${status.capture_preparation}`)}</p>
      <button type="button" className="btn" title={`${t('computer.title')} · ${formatBinding(bindingFor('desktopPanel'))}`} onClick={() => window.dispatchEvent(new Event('hexagon:computer-settings'))}>{t('computer.title')}</button>
    </>}
    {unavailable && <p role="status">{t('computer.modelsUnavailable')}</p>}
    {warnings.length === 1 && <p role="status">{t(`computer.model_${warnings[0].model.vision}`, { role: warnings[0].role.role })}</p>}
    {warnings.length > 1 && <details><summary>{t('computer.modelsNeedReview', { count: warnings.length })}</summary>
      {warnings.map(({ role, model }) => <p key={role.id}>{t(`computer.model_${model.vision}`, { role: role.role })}</p>)}
    </details>}
    {(warnings.length > 0 || unavailable) && <>
      {models && <p>{models.vision_slots.length ? t('computer.visionSlots', { slots: models.vision_slots.join(', ') }) : t('computer.noVisionSlots')}</p>}
      <button type="button" className="btn" title={`${t('computer.modelSettings')} · ${formatBinding(bindingFor('computerModelSettings'))}`} onClick={openModels}>{t('computer.modelSettings')}</button>
    </>}
  </aside>
}
