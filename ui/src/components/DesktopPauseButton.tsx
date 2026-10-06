import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api, errText } from '../api'
import { pauseDesktop, resumeDesktop } from '../desktopPause'
import { bindingFor, formatBinding } from '../keymap'
import { useUiStore } from '../store'
import type { DesktopStatus } from '../gen/DesktopStatus'
import { Icon } from './Icon'

/** Owner 2026-10-06: static Pause/Resume buttons hid disabled and paused state.
 * Read the host state; neither consent nor an owner's pause is changed implicitly. */
export function DesktopPauseButton() {
  const { t } = useTranslation()
  const epoch = useUiStore(state => state.projectEpoch)
  const [read, setRead] = useState<{ epoch: number; status: DesktopStatus } | null>(null)
  const [error, setError] = useState('')
  const [pending, setPending] = useState(false)
  const status = read?.epoch === epoch ? read.status : null
  useEffect(() => {
    let active = true, sequence = 0
    const refresh = async () => {
      const ticket = ++sequence
      try {
        const value = await api.desktopStatus()
        if (active && ticket === sequence) { setRead({ epoch, status: value }); setError('') }
      } catch (e) {
        if (active && ticket === sequence) { setRead(null); setError(errText(e)) }
      }
    }
    void refresh()
    const timer = window.setInterval(() => { if (!document.hidden) void refresh() }, 2000)
    window.addEventListener('focus', refresh)
    window.addEventListener('hexagon:desktop-status-changed', refresh)
    return () => {
      active = false; window.clearInterval(timer)
      window.removeEventListener('focus', refresh)
      window.removeEventListener('hexagon:desktop-status-changed', refresh)
    }
  }, [epoch])
  const action = status?.paused ? 'resume' : 'pause'
  const label = !status ? 'computer.loading' : !status.enabled ? 'computer.disabled' : `computer.${action}`
  const disabled = pending || !status?.enabled || (action === 'resume' && (status.busy || (status.active_project !== null && status.active_project !== status.project_root)))
  return <button type="button" className="btn" disabled={disabled}
    title={error || [t(label), status?.enabled ? formatBinding(bindingFor(action === 'resume' ? 'desktopResume' : 'desktopPause')) : ''].filter(Boolean).join(' · ')}
    onClick={async () => {
      if (disabled) return
      setPending(true)
      try { await (action === 'resume' ? resumeDesktop() : pauseDesktop()) }
      finally { setPending(false) }
    }}>
    <Icon name={status?.enabled ? action === 'pause' ? 'pause' : 'arrow-right' : 'computer'} size={12} /> {t(label)}
  </button>
}
