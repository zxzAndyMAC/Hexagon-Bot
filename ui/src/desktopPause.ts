import { api, errText } from './api'
import i18n from './i18n'
import { useUiStore } from './store'

let pending: Promise<void> | undefined

/** Ticket 07: explicit emergency pause remains available outside the workbench.
 * Bind the command to the fresh host project; switching during IPC fails closed. */
export function pauseDesktop(): Promise<void> {
  if (pending) return pending
  pending = (async () => {
    try {
      const status = await api.desktopStatus()
      await api.desktopControl('pause', status.project_root)
      useUiStore.getState().pushToast(i18n.t('computer.paused'))
    } catch (error) { useUiStore.getState().pushToast(errText(error), 'err') }
  })().finally(() => { pending = undefined })
  return pending
}
