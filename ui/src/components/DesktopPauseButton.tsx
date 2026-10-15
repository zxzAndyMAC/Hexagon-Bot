import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { pauseDesktop, resumeDesktop } from '../desktopPause'
import { bindingFor, formatBinding } from '../keymap'
import { useDesktopStatus } from '../useDesktopStatus'
import { Icon } from './Icon'

/** Owner 2026-10-06: static Pause/Resume buttons hid disabled and paused state.
 * Read the host state; neither consent nor an owner's pause is changed implicitly. */
export function DesktopPauseButton() {
  const { t } = useTranslation()
  // 功能缺口 C4：与头部 chip、面板共用一份宿主读（useDesktopStatus）。
  const { status, error } = useDesktopStatus()
  const [pending, setPending] = useState(false)
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
