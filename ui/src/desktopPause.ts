import { api, errText } from './api'
import i18n from './i18n'
import { useUiStore } from './store'

let pending: Promise<void> | undefined
let resuming: Promise<void> | undefined

/** QA17 (2026-10-05): a rejected action releases its preview. Recovery must
 * still be possible from Settings/global key, using the fresh host root. */
export function resumeDesktop(): Promise<void> {
  if (resuming) return resuming
  resuming = (async () => {
    try {
      const status = await api.desktopStatus()
      // QA readiness 2026-10-06: a global shortcut once bypassed the panel's
      // preparation wait. Keep both recovery paths consistent; no action replay.
      if (status.capture_preparation === 'preparing') {
        useUiStore.getState().pushToast(i18n.t('computer.capture_preparing'))
        return
      }
      await api.desktopControl('resume', status.project_root)
      window.dispatchEvent(new Event('hexagon:desktop-status-changed'))
      useUiStore.getState().pushToast(i18n.t('computer.resumed'))
    } catch (error) { useUiStore.getState().pushToast(errText(error), 'err') }
  })().finally(() => { resuming = undefined })
  return resuming
}

/** Ticket 07: explicit emergency pause remains available outside the workbench.
 * Bind the command to the fresh host project; switching during IPC fails closed. */
export function pauseDesktop(): Promise<void> {
  if (pending) return pending
  pending = (async () => {
    try {
      const status = await api.desktopStatus()
      await api.desktopControl('pause', status.project_root)
      window.dispatchEvent(new Event('hexagon:desktop-status-changed'))
      useUiStore.getState().pushToast(i18n.t('computer.paused'))
    } catch (error) { useUiStore.getState().pushToast(errText(error), 'err') }
  })().finally(() => { pending = undefined })
  return pending
}
